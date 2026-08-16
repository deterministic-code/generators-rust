use serde_json::Value;

use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::mappings::uuid_validator::validate_uuid;
use crate::repositories::datasource_middleware::DatabaseBackend;

pub struct PostgresUuidConverter;

impl TypeFieldConverter for PostgresUuidConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Postgres
    }
    fn datasource_type(&self) -> &str {
        "uuid"
    }
    fn from(&self, value: &Value) -> Value {
        validate_uuid("postgres", "from", value)
    }
    fn to(&self, value: &Value) -> Value {
        validate_uuid("postgres", "to", value)
    }
    fn bind_sql_cast(&self) -> Option<&str> {
        Some("::uuid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn advertises_postgres_and_uuid() {
        let c = PostgresUuidConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Postgres);
        assert_eq!(c.datasource_type(), "uuid");
    }

    #[test]
    fn bind_sql_cast_is_uuid() {
        let c = PostgresUuidConverter;
        assert_eq!(c.bind_sql_cast(), Some("::uuid"));
    }

    #[test]
    fn value_passthrough_in_both_directions() {
        let c = PostgresUuidConverter;
        let s = Value::from("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(c.from(&s), s);
        assert_eq!(c.to(&s), s);
    }

    #[test]
    fn object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(PostgresUuidConverter);
    }

    #[test]
    #[should_panic(expected = "expected RFC-4122 UUID string")]
    fn from_panics_on_non_uuid_string() {
        PostgresUuidConverter.from(&Value::from("not-a-uuid"));
    }

    #[test]
    #[should_panic(expected = "expected RFC-4122 UUID string")]
    fn to_panics_on_non_uuid_string() {
        PostgresUuidConverter.to(&Value::from("not-a-uuid"));
    }
}
