use serde_json::Value;

use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::datasource_middleware::DatabaseBackend;

pub struct PostgresDateTimeConverter;

impl TypeFieldConverter for PostgresDateTimeConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Postgres
    }
    fn datasource_type(&self) -> &str {
        "datetime"
    }
    fn from(&self, value: &Value) -> Value {
        value.clone()
    }
    fn to(&self, value: &Value) -> Value {
        value.clone()
    }
    fn bind_sql_cast(&self) -> Option<&str> {
        Some("::timestamptz")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn advertises_postgres_and_datetime() {
        let c = PostgresDateTimeConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Postgres);
        assert_eq!(c.datasource_type(), "datetime");
    }

    #[test]
    fn bind_sql_cast_is_timestamptz() {
        let c = PostgresDateTimeConverter;
        assert_eq!(c.bind_sql_cast(), Some("::timestamptz"));
    }

    #[test]
    fn value_passthrough_in_both_directions() {
        let c = PostgresDateTimeConverter;
        let s = Value::from("2026-06-09T12:34:56.000Z");
        assert_eq!(c.from(&s), s);
        assert_eq!(c.to(&s), s);
    }

    #[test]
    fn object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(PostgresDateTimeConverter);
    }
}
