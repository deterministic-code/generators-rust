use serde_json::Value;

use crate::mappings::type_field_converter::{binary_bytes_to_string, TypeFieldConverter};
use crate::repositories::datasource_middleware::DatabaseBackend;

// why no bind cast: SQLite BLOB accepts the sqlx-bound TEXT payload — no dialect cast needed, unlike postgres ::bytea.
pub struct SqliteBinaryConverter;

impl TypeFieldConverter for SqliteBinaryConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Sqlite
    }
    fn datasource_type(&self) -> &str {
        "binary"
    }
    // The BLOB column decoder yields the stored bytes as a JSON number array; binary is carried on the wire as a string (OpenAPI `format: byte`), so recover that string from the bytes. `to` is the identity — the wire string is bound straight into the BLOB.
    fn from(&self, value: &Value) -> Value {
        binary_bytes_to_string(value)
    }
    fn to(&self, value: &Value) -> Value {
        value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn advertises_sqlite_and_binary() {
        let c = SqliteBinaryConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Sqlite);
        assert_eq!(c.datasource_type(), "binary");
    }

    #[test]
    fn bind_sql_cast_is_none() {
        let c = SqliteBinaryConverter;
        assert_eq!(c.bind_sql_cast(), None);
    }

    #[test]
    fn value_passthrough_in_both_directions() {
        let c = SqliteBinaryConverter;
        let empty = Value::from("");
        assert_eq!(c.from(&empty), empty);
        assert_eq!(c.to(&empty), empty);
    }

    #[test]
    fn null_passes_through_unchanged() {
        let c = SqliteBinaryConverter;
        assert!(c.from(&Value::Null).is_null());
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(SqliteBinaryConverter);
    }
}
