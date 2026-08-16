use serde_json::{Number, Value};
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo, ValueRef};
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::row_map::RowMap;

// why this trait: row_convert.rs used to be a single big match on sqlx column type names — closed to extension. Custom types (geometry, network ranges, vendor types) had no way in. This is the open extension point: register an Arc<dyn PostgresColumnDecoder> on the datasource and it runs before the builtin chain. Return `Ok(None)` to delegate to the next decoder. The default catch-all handles anything no decoder claimed.
pub trait PostgresColumnDecoder: Send + Sync {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError>;
}

#[derive(Clone)]
pub struct PostgresDecoderRegistry {
    custom: Vec<Arc<dyn PostgresColumnDecoder>>,
    builtin: Vec<Arc<dyn PostgresColumnDecoder>>,
}

impl Default for PostgresDecoderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PostgresDecoderRegistry {
    pub fn new() -> Self {
        Self {
            custom: Vec::new(),
            builtin: builtin_chain(),
        }
    }

    pub fn empty() -> Self {
        Self {
            custom: Vec::new(),
            builtin: Vec::new(),
        }
    }

    pub fn with_decoder(mut self, decoder: Arc<dyn PostgresColumnDecoder>) -> Self {
        self.custom.push(decoder);
        self
    }

    pub fn decode_rows(&self, rows: Vec<PgRow>) -> Result<Vec<RowMap>, RepositoryError> {
        rows.into_iter().map(|r| self.decode_row(r)).collect()
    }

    pub fn decode_row(&self, row: PgRow) -> Result<RowMap, RepositoryError> {
        let mut map = RowMap::new();
        for (i, col) in row.columns().iter().enumerate() {
            let name = col.name().to_string();
            let value = self.decode_column(&row, i)?;
            map.insert(name, value);
        }
        Ok(map)
    }

    fn decode_column(&self, row: &PgRow, idx: usize) -> Result<Value, RepositoryError> {
        let raw = row.try_get_raw(idx).map_err(RepositoryError::Sqlx)?;
        if raw.is_null() {
            return Ok(Value::Null);
        }
        let type_name = raw.type_info().name().to_uppercase();
        for decoder in self.custom.iter().chain(self.builtin.iter()) {
            if let Some(v) = decoder.decode(row, idx, &type_name)? {
                return Ok(v);
            }
        }
        decode_fallback(row, idx)
    }
}

fn builtin_chain() -> Vec<Arc<dyn PostgresColumnDecoder>> {
    vec![
        Arc::new(PostgresDateTimeColumnDecoder),
        Arc::new(PostgresUuidColumnDecoder),
        Arc::new(PostgresBinaryColumnDecoder),
        Arc::new(PostgresBooleanColumnDecoder),
        Arc::new(PostgresNumericColumnDecoder),
        Arc::new(PostgresJsonColumnDecoder),
        Arc::new(PostgresDateColumnDecoder),
        Arc::new(PostgresTimeColumnDecoder),
        Arc::new(PostgresTextColumnDecoder),
    ]
}

// why TIMESTAMPTZ vs TIMESTAMP: TIMESTAMPTZ is UTC-stamped (DateTime<Utc>), TIMESTAMP is naive (NaiveDateTime). Both render to the same canonical "%Y-%m-%dT%H:%M:%S%.3fZ" wire format the OpenAPI schema expects.
pub struct PostgresDateTimeColumnDecoder;

impl PostgresColumnDecoder for PostgresDateTimeColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "TIMESTAMPTZ" => {
                let v: chrono::DateTime<chrono::Utc> =
                    row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::String(
                    v.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                )))
            }
            "TIMESTAMP" => {
                let v: chrono::NaiveDateTime = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::String(
                    v.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                )))
            }
            _ => Ok(None),
        }
    }
}

pub struct PostgresUuidColumnDecoder;

impl PostgresColumnDecoder for PostgresUuidColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        if type_name != "UUID" {
            return Ok(None);
        }
        let v: uuid::Uuid = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
        Ok(Some(Value::String(v.hyphenated().to_string())))
    }
}

pub struct PostgresBinaryColumnDecoder;

impl PostgresColumnDecoder for PostgresBinaryColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        if type_name != "BYTEA" {
            return Ok(None);
        }
        let v: Vec<u8> = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
        Ok(Some(Value::Array(
            v.into_iter().map(|b| Value::Number(b.into())).collect(),
        )))
    }
}

pub struct PostgresBooleanColumnDecoder;

impl PostgresColumnDecoder for PostgresBooleanColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        if type_name != "BOOL" {
            return Ok(None);
        }
        let v: bool = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
        Ok(Some(Value::Bool(v)))
    }
}

// why one decoder for many numeric types: int + float + numeric cells all collapse to JSON Number, but each PG wire type binds to a distinct Rust scalar (i16/i32/i64/f32/f64). One struct keeps the type-name → scalar mapping in one place. NUMERIC binds as f64 (lossy) — preserves prior row_convert behavior; callers needing arbitrary precision register a custom decoder.
pub struct PostgresNumericColumnDecoder;

impl PostgresColumnDecoder for PostgresNumericColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "INT2" | "SMALLINT" => {
                let v: i16 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number((v as i64).into())))
            }
            "INT4" | "INTEGER" | "INT" => {
                let v: i32 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number((v as i64).into())))
            }
            "INT8" | "BIGINT" => {
                let v: i64 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number(v.into())))
            }
            "FLOAT4" | "REAL" => {
                let v: f32 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(
                    Number::from_f64(v as f64)
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                ))
            }
            "FLOAT8" | "DOUBLE PRECISION" | "NUMERIC" => {
                let v: f64 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(
                    Number::from_f64(v)
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                ))
            }
            _ => Ok(None),
        }
    }
}

pub struct PostgresJsonColumnDecoder;

impl PostgresColumnDecoder for PostgresJsonColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "JSON" | "JSONB" => {
                let v: Value = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(v))
            }
            _ => Ok(None),
        }
    }
}

pub struct PostgresDateColumnDecoder;

impl PostgresColumnDecoder for PostgresDateColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        if type_name != "DATE" {
            return Ok(None);
        }
        let v: chrono::NaiveDate = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
        Ok(Some(Value::String(v.format("%Y-%m-%d").to_string())))
    }
}

pub struct PostgresTimeColumnDecoder;

impl PostgresColumnDecoder for PostgresTimeColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        if type_name != "TIME" {
            return Ok(None);
        }
        let v: chrono::NaiveTime = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
        Ok(Some(Value::String(v.format("%H:%M:%S%.3f").to_string())))
    }
}

// why these five type_names: sqlx-postgres maps the SQL TEXT/VARCHAR/CHAR family + the system-catalog NAME type to their pg_type names without size info — explicit promotion lets users override string handling via `with_decoder` without writing one decoder per concrete column.
pub struct PostgresTextColumnDecoder;

impl PostgresColumnDecoder for PostgresTextColumnDecoder {
    fn decode(
        &self,
        row: &PgRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "TEXT" | "VARCHAR" | "BPCHAR" | "CHAR" | "NAME" => {
                let v: String = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::String(v)))
            }
            _ => Ok(None),
        }
    }
}

// why kept private: cells whose type_name no decoder claimed land here as a String. With Text/Boolean/Numeric/Json/Date/Time/DateTime/Uuid/Binary all promoted to decoders, the only fallthroughs are exotic / vendor types (geometry, network ranges, custom enums); if sqlx can't bind the cell to String, it surfaces a typed RepositoryError::Sqlx rather than silently corrupting the row.
fn decode_fallback(row: &PgRow, idx: usize) -> Result<Value, RepositoryError> {
    let v: String = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
    Ok(Value::String(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_seeds_builtin_decoders() {
        let r = PostgresDecoderRegistry::new();
        assert_eq!(
            r.builtin.len(),
            9,
            "datetime + uuid + binary + boolean + numeric + json + date + time + text wired by default"
        );
        assert!(r.custom.is_empty());
    }

    #[test]
    fn empty_registry_has_no_builtin_decoders() {
        let r = PostgresDecoderRegistry::empty();
        assert!(r.builtin.is_empty());
        assert!(r.custom.is_empty());
    }

    #[test]
    fn with_decoder_appends_to_custom_chain() {
        struct StubDecoder;
        impl PostgresColumnDecoder for StubDecoder {
            fn decode(
                &self,
                _row: &PgRow,
                _idx: usize,
                _type_name: &str,
            ) -> Result<Option<Value>, RepositoryError> {
                Ok(None)
            }
        }
        let r = PostgresDecoderRegistry::new().with_decoder(Arc::new(StubDecoder));
        assert_eq!(r.custom.len(), 1, "custom chain holds user-added decoders");
        assert_eq!(
            r.builtin.len(),
            9,
            "with_decoder must not displace the builtin chain"
        );
    }

    #[test]
    fn registry_is_clone_to_share_across_pool_handles() {
        let r = PostgresDecoderRegistry::new();
        let _clone = r.clone();
    }
}
