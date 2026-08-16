use serde_json::Value;

use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::mappings::uuid_validator::validate_uuid;
use crate::repositories::datasource_middleware::DatabaseBackend;

// why no bind cast: MySQL stores UUIDs as CHAR(36) so sqlx binds a `Value::String` straight in — no dialect cast needed (unlike postgres ::uuid).
pub struct MysqlUuidConverter;

impl TypeFieldConverter for MysqlUuidConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Mysql
    }
    fn datasource_type(&self) -> &str {
        "uuid"
    }
    fn from(&self, value: &Value) -> Value {
        validate_uuid("mysql", "from", value)
    }
    fn to(&self, value: &Value) -> Value {
        validate_uuid("mysql", "to", value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn advertises_mysql_and_uuid() {
        let c = MysqlUuidConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Mysql);
        assert_eq!(c.datasource_type(), "uuid");
    }

    #[test]
    fn bind_sql_cast_is_none() {
        let c = MysqlUuidConverter;
        assert_eq!(c.bind_sql_cast(), None);
    }

    #[test]
    fn value_passthrough_in_both_directions() {
        let c = MysqlUuidConverter;
        let s = Value::from("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(c.from(&s), s);
        assert_eq!(c.to(&s), s);
    }

    #[test]
    fn null_passes_through_unchanged() {
        let c = MysqlUuidConverter;
        assert!(c.from(&Value::Null).is_null());
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(MysqlUuidConverter);
    }

    #[test]
    #[should_panic(expected = "expected RFC-4122 UUID string")]
    fn from_panics_on_non_uuid_string() {
        MysqlUuidConverter.from(&Value::from("not-a-uuid"));
    }

    #[test]
    #[should_panic(expected = "expected RFC-4122 UUID string")]
    fn to_panics_on_non_uuid_string() {
        MysqlUuidConverter.to(&Value::from("not-a-uuid"));
    }
}
