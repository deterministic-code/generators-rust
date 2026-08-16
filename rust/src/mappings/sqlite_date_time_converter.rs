use serde_json::Value;

use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::datasource_middleware::DatabaseBackend;

// why pass-through: SQLite stores datetime as ISO 8601 TEXT — the transport JSON form is already valid for sqlx bind (unlike MySQL DATETIME, which rejects the ISO `T`/`Z`).
pub struct SqliteDateTimeConverter;

impl TypeFieldConverter for SqliteDateTimeConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Sqlite
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn advertises_sqlite_and_datetime() {
        let c = SqliteDateTimeConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Sqlite);
        assert_eq!(c.datasource_type(), "datetime");
    }

    #[test]
    fn bind_sql_cast_is_none() {
        let c = SqliteDateTimeConverter;
        assert_eq!(c.bind_sql_cast(), None);
    }

    #[test]
    fn value_passthrough_in_both_directions() {
        let c = SqliteDateTimeConverter;
        let s = Value::from("2024-01-01T00:00:00.000Z");
        assert_eq!(c.from(&s), s);
        assert_eq!(c.to(&s), s);
    }

    #[test]
    fn null_passes_through_unchanged() {
        let c = SqliteDateTimeConverter;
        assert!(c.from(&Value::Null).is_null());
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(SqliteDateTimeConverter);
    }
}
