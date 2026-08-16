use serde_json::Value;

use crate::mappings::type_field_converter::{binary_bytes_to_string, TypeFieldConverter};
use crate::repositories::datasource_middleware::DatabaseBackend;

// why ::bytea bind cast: sqlx binds Value::String as TEXT and Postgres won't implicitly cast text→bytea, so an empty/base64 string into BYTEA 500s without the cast.
pub struct PostgresBinaryConverter;

impl TypeFieldConverter for PostgresBinaryConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Postgres
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
    fn bind_sql_cast(&self) -> Option<&str> {
        Some("::bytea")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn advertises_postgres_and_binary() {
        let c = PostgresBinaryConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Postgres);
        assert_eq!(c.datasource_type(), "binary");
    }

    #[test]
    fn bind_sql_cast_is_bytea() {
        let c = PostgresBinaryConverter;
        assert_eq!(c.bind_sql_cast(), Some("::bytea"));
    }

    #[test]
    fn value_passthrough_in_both_directions() {
        let c = PostgresBinaryConverter;
        let empty = Value::from("");
        assert_eq!(c.from(&empty), empty);
        assert_eq!(c.to(&empty), empty);
    }

    #[test]
    fn object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(PostgresBinaryConverter);
    }
}
