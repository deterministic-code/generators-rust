use serde_json::Value;

use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::datasource_middleware::DatabaseBackend;

pub struct MysqlDateTimeConverter;

impl TypeFieldConverter for MysqlDateTimeConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Mysql
    }
    fn datasource_type(&self) -> &str {
        "datetime"
    }
    // why normalize: sqlx binds JSON strings as TEXT; MySQL DATETIME parses
    // "YYYY-MM-DD HH:MM:SS[.fff]" but rejects ISO "YYYY-MM-DDTHH:MM:SS.fffZ"
    // (1292 Incorrect datetime value). Strip the `T` and trailing `Z` so the
    // server-side coercion succeeds.
    fn to(&self, value: &Value) -> Value {
        match value.as_str() {
            Some(s) => Value::from(normalize_iso_to_mysql_datetime(s)),
            None => value.clone(),
        }
    }
    fn from(&self, value: &Value) -> Value {
        value.clone()
    }
}

fn normalize_iso_to_mysql_datetime(s: &str) -> String {
    let mut out = s.replacen('T', " ", 1);
    if out.ends_with('Z') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn advertises_mysql_and_datetime() {
        let c = MysqlDateTimeConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Mysql);
        assert_eq!(c.datasource_type(), "datetime");
    }

    #[test]
    fn to_strips_iso_t_and_trailing_z() {
        let c = MysqlDateTimeConverter;
        let out = c.to(&Value::from("2024-01-01T00:00:00.000Z"));
        assert_eq!(out.as_str(), Some("2024-01-01 00:00:00.000"));
    }

    #[test]
    fn to_handles_iso_without_millis() {
        let c = MysqlDateTimeConverter;
        let out = c.to(&Value::from("2024-01-01T00:00:00Z"));
        assert_eq!(out.as_str(), Some("2024-01-01 00:00:00"));
    }

    #[test]
    fn to_leaves_mysql_format_alone() {
        let c = MysqlDateTimeConverter;
        let out = c.to(&Value::from("2024-01-01 00:00:00"));
        assert_eq!(out.as_str(), Some("2024-01-01 00:00:00"));
    }

    #[test]
    fn to_passes_through_null() {
        let c = MysqlDateTimeConverter;
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn from_is_passthrough() {
        let c = MysqlDateTimeConverter;
        let s = Value::from("2024-01-01T00:00:00.000Z");
        assert_eq!(c.from(&s), s);
    }

    #[test]
    fn bind_sql_cast_is_none() {
        let c = MysqlDateTimeConverter;
        assert_eq!(c.bind_sql_cast(), None);
    }

    #[test]
    fn object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(MysqlDateTimeConverter);
    }
}
