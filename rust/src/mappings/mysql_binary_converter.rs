use serde_json::Value;

use crate::mappings::type_field_converter::{binary_bytes_to_string, TypeFieldConverter};
use crate::repositories::datasource_middleware::DatabaseBackend;

// why no bind cast: MySQL VARBINARY(n) / LONGBLOB accept the sqlx-bound TEXT payload the emitters produce (empty string or base64) — no dialect cast needed, unlike postgres ::bytea.
pub struct MysqlBinaryConverter;

impl TypeFieldConverter for MysqlBinaryConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Mysql
    }
    fn datasource_type(&self) -> &str {
        "binary"
    }
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
    fn advertises_mysql_and_binary() {
        let c = MysqlBinaryConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Mysql);
        assert_eq!(c.datasource_type(), "binary");
    }

    #[test]
    fn bind_sql_cast_is_none() {
        let c = MysqlBinaryConverter;
        assert_eq!(c.bind_sql_cast(), None);
    }

    #[test]
    fn value_passthrough_in_both_directions() {
        let c = MysqlBinaryConverter;
        let empty = Value::from("");
        assert_eq!(c.from(&empty), empty);
        assert_eq!(c.to(&empty), empty);
    }

    #[test]
    fn null_passes_through_unchanged() {
        let c = MysqlBinaryConverter;
        assert!(c.from(&Value::Null).is_null());
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(MysqlBinaryConverter);
    }
}
