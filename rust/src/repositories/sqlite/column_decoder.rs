use serde_json::{Number, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Column, Row, TypeInfo, ValueRef};
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::row_map::RowMap;

// why this trait: row_convert.rs used to be a single big match on sqlx column type names — closed to extension. Custom types (geometry, network ranges, vendor types) had no way in. This is the open extension point: register an Arc<dyn SqliteColumnDecoder> on the datasource and it runs before the builtin chain. Return `Ok(None)` to delegate to the next decoder. The default catch-all handles anything no decoder claimed.
pub trait SqliteColumnDecoder: Send + Sync {
    fn decode(
        &self,
        row: &SqliteRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError>;
}

#[derive(Clone)]
pub struct SqliteDecoderRegistry {
    custom: Vec<Arc<dyn SqliteColumnDecoder>>,
    builtin: Vec<Arc<dyn SqliteColumnDecoder>>,
}

impl Default for SqliteDecoderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SqliteDecoderRegistry {
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

    pub fn with_decoder(mut self, decoder: Arc<dyn SqliteColumnDecoder>) -> Self {
        self.custom.push(decoder);
        self
    }

    pub fn decode_rows(&self, rows: Vec<SqliteRow>) -> Result<Vec<RowMap>, RepositoryError> {
        rows.into_iter().map(|r| self.decode_row(r)).collect()
    }

    pub fn decode_row(&self, row: SqliteRow) -> Result<RowMap, RepositoryError> {
        // why two type names: SQLite distinguishes the *declared* column type (from CREATE TABLE) from the *storage* type (sqlx's reported type info for the actual cell — INTEGER/TEXT/REAL/BLOB). Declared type takes precedence when present so a column declared as BOOLEAN still decodes to bool even if sqlite stored it as INTEGER 0/1. Falls back to storage type for synthetic columns like COUNT(*).
        let columns = row.columns();
        let count = columns.len();
        let mut declared: Vec<String> = Vec::with_capacity(count);
        let mut names: Vec<String> = Vec::with_capacity(count);
        for col in columns.iter() {
            declared.push(col.type_info().name().to_uppercase());
            names.push(col.name().to_string());
        }
        let mut map = RowMap::new();
        for (i, name) in names.into_iter().enumerate() {
            let value = self.decode_column(&row, i, &declared[i])?;
            map.insert(name, value);
        }
        Ok(map)
    }

    fn decode_column(
        &self,
        row: &SqliteRow,
        idx: usize,
        declared_type: &str,
    ) -> Result<Value, RepositoryError> {
        let raw = row.try_get_raw(idx).map_err(RepositoryError::Sqlx)?;
        if raw.is_null() {
            return Ok(Value::Null);
        }
        let storage = raw.type_info().name().to_uppercase();
        let effective: &str = if declared_type.is_empty() || declared_type == "NULL" {
            storage.as_str()
        } else {
            declared_type
        };
        for decoder in self.custom.iter().chain(self.builtin.iter()) {
            if let Some(v) = decoder.decode(row, idx, effective)? {
                return Ok(v);
            }
        }
        decode_fallback(row, idx)
    }
}

fn builtin_chain() -> Vec<Arc<dyn SqliteColumnDecoder>> {
    // why this set: datetime + uuid stay in the String catch-all (SQLite stores them as TEXT). The rest — BLOB, BOOLEAN-aliases, INTEGER/REAL family — each map to a distinct non-String JSON shape and want their own decoder.
    vec![
        Arc::new(SqliteBinaryColumnDecoder),
        Arc::new(SqliteBooleanColumnDecoder),
        Arc::new(SqliteNumericColumnDecoder),
    ]
}

pub struct SqliteBinaryColumnDecoder;

impl SqliteColumnDecoder for SqliteBinaryColumnDecoder {
    fn decode(
        &self,
        row: &SqliteRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        if type_name != "BLOB" {
            return Ok(None);
        }
        let v: Vec<u8> = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
        Ok(Some(Value::Array(
            v.into_iter().map(|b| Value::Number(b.into())).collect(),
        )))
    }
}

pub struct SqliteBooleanColumnDecoder;

impl SqliteColumnDecoder for SqliteBooleanColumnDecoder {
    fn decode(
        &self,
        row: &SqliteRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "BOOLEAN" | "BOOL" => {
                let v: bool = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Bool(v)))
            }
            _ => Ok(None),
        }
    }
}

// why one decoder for many numeric types: SQLite collapses everything to INTEGER or REAL storage classes, but declared types let callers tag a column INT / BIGINT / FLOAT / etc. Group them so the int-vs-float split is a single match instead of two micro-decoders.
pub struct SqliteNumericColumnDecoder;

impl SqliteColumnDecoder for SqliteNumericColumnDecoder {
    fn decode(
        &self,
        row: &SqliteRow,
        idx: usize,
        type_name: &str,
    ) -> Result<Option<Value>, RepositoryError> {
        match type_name {
            "INTEGER" | "INT" | "BIGINT" => {
                let v: i64 = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                Ok(Some(Value::Number(v.into())))
            }
            "REAL" | "FLOAT" | "DOUBLE" | "NUMERIC" => {
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

// why kept private: every cell that no decoder above claimed lands here as a String. Preserves the prior row_convert behavior for TEXT plus the long tail (datetime stored as ISO 8601, uuid stored as RFC-4122, and any synthetic columns sqlx reports without a declared type).
fn decode_fallback(row: &SqliteRow, idx: usize) -> Result<Value, RepositoryError> {
    let v: String = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
    Ok(Value::String(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_seeds_builtin_decoders() {
        let r = SqliteDecoderRegistry::new();
        assert_eq!(
            r.builtin.len(),
            3,
            "binary + boolean + numeric wired by default"
        );
        assert!(r.custom.is_empty());
    }

    #[test]
    fn empty_registry_has_no_builtin_decoders() {
        let r = SqliteDecoderRegistry::empty();
        assert!(r.builtin.is_empty());
        assert!(r.custom.is_empty());
    }

    #[test]
    fn with_decoder_appends_to_custom_chain() {
        struct StubDecoder;
        impl SqliteColumnDecoder for StubDecoder {
            fn decode(
                &self,
                _row: &SqliteRow,
                _idx: usize,
                _type_name: &str,
            ) -> Result<Option<Value>, RepositoryError> {
                Ok(None)
            }
        }
        let r = SqliteDecoderRegistry::new().with_decoder(Arc::new(StubDecoder));
        assert_eq!(r.custom.len(), 1, "custom chain holds user-added decoders");
        assert_eq!(
            r.builtin.len(),
            3,
            "with_decoder must not displace the builtin chain"
        );
    }

    #[test]
    fn registry_is_clone_to_share_across_pool_handles() {
        let r = SqliteDecoderRegistry::new();
        let _clone = r.clone();
    }
}
