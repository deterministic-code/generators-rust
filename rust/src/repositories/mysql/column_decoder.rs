use serde_json::{Number, Value};
use sqlx::mysql::MySqlRow;
use sqlx::{Column, Row, TypeInfo, ValueRef};
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::row_map::RowMap;

// why this trait: row_convert.rs used to be a single big match on sqlx column type names — closed to extension. Custom types (geometry, network ranges, vendor types) had no way in. This is the open extension point: register an Arc<dyn MysqlColumnDecoder> on the datasource and it runs before the builtin chain. Return `Ok(None)` to delegate to the next decoder. The default catch-all handles anything no decoder claimed.
pub trait MysqlColumnDecoder: Send + Sync {
    fn decode(
        &self,
        row: &MySqlRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError>;
}

#[derive(Clone)]
pub struct MysqlDecoderRegistry {
    custom: Vec<Arc<dyn MysqlColumnDecoder>>,
    builtin: Vec<Arc<dyn MysqlColumnDecoder>>,
}

impl Default for MysqlDecoderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MysqlDecoderRegistry {
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

    pub fn with_decoder(mut self, decoder: Arc<dyn MysqlColumnDecoder>) -> Self {
        self.custom.push(decoder);
        self
    }

    pub fn decode_rows(&self, rows: Vec<MySqlRow>) -> Result<Vec<RowMap>, RepositoryError> {
        rows.into_iter().map(|r| self.decode_row(r)).collect()
    }

    pub fn decode_row(&self, row: MySqlRow) -> Result<RowMap, RepositoryError> {
        let mut map = RowMap::new();
        for (i, col) in row.columns().iter().enumerate() {
            let name = col.name().to_string();
            let value = self.decode_column(&row, i)?;
            map.insert(name, value);
        }
        Ok(map)
    }

    fn decode_column(&self, row: &MySqlRow, idx: usize) -> Result<Value, RepositoryError> {
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

fn builtin_chain() -> Vec<Arc<dyn MysqlColumnDecoder>> {
    vec![
        Arc::new(MysqlDateTimeColumnDecoder),
        Arc::new(MysqlBinaryColumnDecoder),
        Arc::new(MysqlBooleanColumnDecoder),
        Arc::new(MysqlNumericColumnDecoder),
        Arc::new(MysqlJsonColumnDecoder),
        Arc::new(MysqlDateColumnDecoder),
        Arc::new(MysqlTimeColumnDecoder),
        Arc::new(MysqlTextColumnDecoder),
    ]
}

// why DATETIME vs TIMESTAMP: MySQL stores DATETIME without TZ (NaiveDateTime), TIMESTAMP with UTC (DateTime<Utc>). Both render to the same canonical "%Y-%m-%dT%H:%M:%S%.3fZ" wire format the OpenAPI schema expects.
pub struct MysqlDateTimeColumnDecoder;

impl MysqlColumnDecoder for MysqlDateTimeColumnDecoder {
    fn decode(
        &self,
        row: &MySqlRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "DATETIME" => {
                let v: chrono::NaiveDateTime = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::String(
                    v.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                )))
            }
            "TIMESTAMP" => {
                let v: chrono::DateTime<chrono::Utc> =
                    row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::String(
                    v.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                )))
            }
            _ => Ok(None),
        }
    }
}

// why all six BLOB-family names: sqlx-mysql reports the column type at the wire-protocol level (BLOB, VARBINARY, BINARY, LONGBLOB, MEDIUMBLOB, TINYBLOB). All six decode the same way — bytes → Value::Array of numbers — but the dispatcher only matches what's listed here.
pub struct MysqlBinaryColumnDecoder;

impl MysqlColumnDecoder for MysqlBinaryColumnDecoder {
    fn decode(
        &self,
        row: &MySqlRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "BLOB" | "VARBINARY" | "BINARY" | "LONGBLOB" | "MEDIUMBLOB" | "TINYBLOB" => {
                let v: Vec<u8> = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Array(
                    v.into_iter().map(|b| Value::Number(b.into())).collect(),
                )))
            }
            _ => Ok(None),
        }
    }
}

// why TINYINT folds into boolean: MySQL has no native BOOLEAN — TINYINT(1) is the convention sqlx reports as `TINYINT`. Treating every TINYINT as bool matches the prior row_convert behavior and the MysqlBooleanConverter typing on the write side.
pub struct MysqlBooleanColumnDecoder;

impl MysqlColumnDecoder for MysqlBooleanColumnDecoder {
    fn decode(
        &self,
        row: &MySqlRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "BOOLEAN" | "TINYINT" => {
                let v: bool = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Bool(v)))
            }
            _ => Ok(None),
        }
    }
}

// why one decoder for many numeric types: integer + float + decimal cells all collapse to JSON Number, but each MySQL wire type binds to a distinct Rust scalar (i16/u16/i32/u32/i64/u64/f32/f64). One struct keeps the type-name → scalar mapping in one place rather than scattering five micro-decoders. `LAST_INSERT_ID()` and AUTO_INCREMENT columns come back as `BIGINT UNSIGNED`.
pub struct MysqlNumericColumnDecoder;

impl MysqlColumnDecoder for MysqlNumericColumnDecoder {
    fn decode(
        &self,
        row: &MySqlRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "TINYINT UNSIGNED" => {
                let v: u8 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number((v as i64).into())))
            }
            "SMALLINT" => {
                let v: i16 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number((v as i64).into())))
            }
            "SMALLINT UNSIGNED" => {
                let v: u16 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number((v as i64).into())))
            }
            "INT" | "INTEGER" | "MEDIUMINT" => {
                let v: i32 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number((v as i64).into())))
            }
            "INT UNSIGNED" | "INTEGER UNSIGNED" | "MEDIUMINT UNSIGNED" => {
                let v: u32 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number((v as i64).into())))
            }
            "BIGINT" => {
                let v: i64 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number(v.into())))
            }
            "BIGINT UNSIGNED" => {
                let v: u64 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number(v.into())))
            }
            "FLOAT" => {
                let v: f32 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(
                    Number::from_f64(v as f64)
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                ))
            }
            "DOUBLE" | "DECIMAL" => {
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

pub struct MysqlJsonColumnDecoder;

impl MysqlColumnDecoder for MysqlJsonColumnDecoder {
    fn decode(
        &self,
        row: &MySqlRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        if type_name != "JSON" {
            return Ok(None);
        }
        let v: Value = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
        Ok(Some(v))
    }
}

pub struct MysqlDateColumnDecoder;

impl MysqlColumnDecoder for MysqlDateColumnDecoder {
    fn decode(
        &self,
        row: &MySqlRow,
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

pub struct MysqlTimeColumnDecoder;

impl MysqlColumnDecoder for MysqlTimeColumnDecoder {
    fn decode(
        &self,
        row: &MySqlRow,
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

// why all eight text type_names: sqlx-mysql reports the concrete column type without size (VARCHAR not VARCHAR(255)), so all string-flavored cells are explicit here — text/string overrides can now register via `with_decoder` instead of falling through to the unknown-type catch-all.
pub struct MysqlTextColumnDecoder;

impl MysqlColumnDecoder for MysqlTextColumnDecoder {
    fn decode(
        &self,
        row: &MySqlRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "CHAR" | "VARCHAR" | "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "ENUM"
            | "SET" => {
                let v: String = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::String(v)))
            }
            _ => Ok(None),
        }
    }
}

// why kept private: cells whose type_name no decoder claimed land here as a String. With Text/Boolean/Numeric/Json/Date/Time/DateTime/Binary all promoted to decoders, the only fallthroughs are exotic vendor types (geometry, MySQL spatial, etc.) — sqlx will error if try_get::<String> doesn't work, so unknown-binary-shaped types still surface a typed RepositoryError::Sqlx rather than silent corruption.
fn decode_fallback(row: &MySqlRow, idx: usize) -> Result<Value, RepositoryError> {
    let v: String = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
    Ok(Value::String(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_seeds_builtin_decoders() {
        let r = MysqlDecoderRegistry::new();
        assert_eq!(
            r.builtin.len(),
            8,
            "datetime + binary + boolean + numeric + json + date + time + text wired by default"
        );
        assert!(r.custom.is_empty());
    }

    #[test]
    fn empty_registry_has_no_builtin_decoders() {
        let r = MysqlDecoderRegistry::empty();
        assert!(r.builtin.is_empty());
        assert!(r.custom.is_empty());
    }

    #[test]
    fn with_decoder_appends_to_custom_chain() {
        struct StubDecoder;
        impl MysqlColumnDecoder for StubDecoder {
            fn decode(
                &self,
                _row: &MySqlRow,
                _idx: usize,
                _type_name: &str,
            ) -> Result<Option<Value>, RepositoryError> {
                Ok(None)
            }
        }
        let r = MysqlDecoderRegistry::new().with_decoder(Arc::new(StubDecoder));
        assert_eq!(r.custom.len(), 1, "custom chain holds user-added decoders");
        assert_eq!(
            r.builtin.len(),
            8,
            "with_decoder must not displace the builtin chain"
        );
    }

    #[test]
    fn registry_is_clone_to_share_across_pool_handles() {
        let r = MysqlDecoderRegistry::new();
        let _clone = r.clone();
    }
}
