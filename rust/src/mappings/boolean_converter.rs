use serde_json::Value;

use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::datasource_middleware::DatabaseBackend;

// why integer-backed: mysql stores BOOLEAN as TINYINT(1); sqlite has no native bool. sqlx may surface either Number(0|1) or Bool(b) depending on the read path. Both directions normalize so callers always see Bool, and writes always serialize as 0/1 for the driver.
fn integer_backed_from(datasource: &str, value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(b) => Value::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(0) => Value::Bool(false),
            Some(1) => Value::Bool(true),
            _ => panic!(
                "booleanConverter[{}].from expected 0 or 1, got {}",
                datasource, n
            ),
        },
        other => panic!(
            "booleanConverter[{}].from expected boolean or 0/1, got {}",
            datasource, other
        ),
    }
}

fn integer_backed_to(datasource: &str, value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(true) => Value::Number(1.into()),
        Value::Bool(false) => Value::Number(0.into()),
        other => panic!(
            "booleanConverter[{}].to expected boolean, got {}",
            datasource, other
        ),
    }
}

fn native_to(datasource: &str, value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(b) => Value::Bool(*b),
        other => panic!(
            "booleanConverter[{}].to expected boolean, got {}",
            datasource, other
        ),
    }
}

fn native_from(datasource: &str, value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(b) => Value::Bool(*b),
        other => panic!(
            "booleanConverter[{}].from expected boolean, got {}",
            datasource, other
        ),
    }
}

pub struct MysqlBooleanConverter;

impl TypeFieldConverter for MysqlBooleanConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Mysql
    }
    fn datasource_type(&self) -> &str {
        "boolean"
    }
    fn from(&self, value: &Value) -> Value {
        integer_backed_from("mysql", value)
    }
    fn to(&self, value: &Value) -> Value {
        integer_backed_to("mysql", value)
    }
}

pub struct SqliteBooleanConverter;

impl TypeFieldConverter for SqliteBooleanConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Sqlite
    }
    fn datasource_type(&self) -> &str {
        "boolean"
    }
    fn from(&self, value: &Value) -> Value {
        integer_backed_from("sqlite", value)
    }
    fn to(&self, value: &Value) -> Value {
        integer_backed_to("sqlite", value)
    }
}

pub struct PostgresBooleanConverter;

impl TypeFieldConverter for PostgresBooleanConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Postgres
    }
    fn datasource_type(&self) -> &str {
        "boolean"
    }
    fn from(&self, value: &Value) -> Value {
        native_from("postgres", value)
    }
    fn to(&self, value: &Value) -> Value {
        native_to("postgres", value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn mysql_advertises_mysql_and_boolean() {
        let c = MysqlBooleanConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Mysql);
        assert_eq!(c.datasource_type(), "boolean");
    }

    #[test]
    fn mysql_from_promotes_number_zero_to_bool_false() {
        let c = MysqlBooleanConverter;
        assert_eq!(c.from(&Value::from(0)), Value::Bool(false));
    }

    #[test]
    fn mysql_from_promotes_number_one_to_bool_true() {
        let c = MysqlBooleanConverter;
        assert_eq!(c.from(&Value::from(1)), Value::Bool(true));
    }

    #[test]
    fn mysql_from_passes_bool_through_unchanged() {
        let c = MysqlBooleanConverter;
        assert_eq!(c.from(&Value::Bool(true)), Value::Bool(true));
        assert_eq!(c.from(&Value::Bool(false)), Value::Bool(false));
    }

    #[test]
    fn mysql_from_passes_null_unchanged() {
        let c = MysqlBooleanConverter;
        assert!(c.from(&Value::Null).is_null());
    }

    #[test]
    #[should_panic(expected = "expected 0 or 1")]
    fn mysql_from_panics_on_unexpected_number() {
        MysqlBooleanConverter.from(&Value::from(7));
    }

    #[test]
    #[should_panic(expected = "expected boolean or 0/1")]
    fn mysql_from_panics_on_string() {
        MysqlBooleanConverter.from(&Value::from("true"));
    }

    #[test]
    #[should_panic(expected = "expected boolean")]
    fn mysql_to_panics_on_non_bool() {
        MysqlBooleanConverter.to(&Value::from(1));
    }

    #[test]
    fn mysql_to_demotes_bool_true_to_number_one() {
        let c = MysqlBooleanConverter;
        assert_eq!(c.to(&Value::Bool(true)), Value::from(1));
    }

    #[test]
    fn mysql_to_demotes_bool_false_to_number_zero() {
        let c = MysqlBooleanConverter;
        assert_eq!(c.to(&Value::Bool(false)), Value::from(0));
    }

    #[test]
    fn mysql_to_passes_null_unchanged() {
        let c = MysqlBooleanConverter;
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn mysql_bind_sql_cast_is_none() {
        let c = MysqlBooleanConverter;
        assert_eq!(c.bind_sql_cast(), None);
    }

    #[test]
    fn mysql_object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(MysqlBooleanConverter);
    }

    #[test]
    fn sqlite_advertises_sqlite_and_boolean() {
        let c = SqliteBooleanConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Sqlite);
        assert_eq!(c.datasource_type(), "boolean");
    }

    #[test]
    fn sqlite_from_promotes_number_zero_to_bool_false() {
        let c = SqliteBooleanConverter;
        assert_eq!(c.from(&Value::from(0)), Value::Bool(false));
    }

    #[test]
    fn sqlite_from_promotes_number_one_to_bool_true() {
        let c = SqliteBooleanConverter;
        assert_eq!(c.from(&Value::from(1)), Value::Bool(true));
    }

    #[test]
    fn sqlite_from_passes_bool_through_unchanged() {
        let c = SqliteBooleanConverter;
        assert_eq!(c.from(&Value::Bool(true)), Value::Bool(true));
    }

    #[test]
    fn sqlite_from_passes_null_unchanged() {
        let c = SqliteBooleanConverter;
        assert!(c.from(&Value::Null).is_null());
    }

    #[test]
    fn sqlite_to_demotes_bool_true_to_number_one() {
        let c = SqliteBooleanConverter;
        assert_eq!(c.to(&Value::Bool(true)), Value::from(1));
    }

    #[test]
    fn sqlite_to_demotes_bool_false_to_number_zero() {
        let c = SqliteBooleanConverter;
        assert_eq!(c.to(&Value::Bool(false)), Value::from(0));
    }

    #[test]
    fn sqlite_to_passes_null_unchanged() {
        let c = SqliteBooleanConverter;
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn sqlite_bind_sql_cast_is_none() {
        let c = SqliteBooleanConverter;
        assert_eq!(c.bind_sql_cast(), None);
    }

    #[test]
    fn sqlite_object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(SqliteBooleanConverter);
    }

    #[test]
    #[should_panic(expected = "expected 0 or 1")]
    fn sqlite_from_panics_on_unexpected_number() {
        SqliteBooleanConverter.from(&Value::from(7));
    }

    #[test]
    #[should_panic(expected = "expected boolean")]
    fn sqlite_to_panics_on_non_bool() {
        SqliteBooleanConverter.to(&Value::from(1));
    }

    #[test]
    fn postgres_advertises_postgres_and_boolean() {
        let c = PostgresBooleanConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Postgres);
        assert_eq!(c.datasource_type(), "boolean");
    }

    #[test]
    fn postgres_from_passes_bool_through_unchanged() {
        let c = PostgresBooleanConverter;
        assert_eq!(c.from(&Value::Bool(true)), Value::Bool(true));
        assert_eq!(c.from(&Value::Bool(false)), Value::Bool(false));
    }

    #[test]
    fn postgres_to_passes_bool_through_unchanged() {
        let c = PostgresBooleanConverter;
        assert_eq!(c.to(&Value::Bool(true)), Value::Bool(true));
        assert_eq!(c.to(&Value::Bool(false)), Value::Bool(false));
    }

    #[test]
    fn postgres_passes_null_through_in_both_directions() {
        let c = PostgresBooleanConverter;
        assert!(c.from(&Value::Null).is_null());
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn postgres_bind_sql_cast_is_none() {
        let c = PostgresBooleanConverter;
        assert_eq!(c.bind_sql_cast(), None);
    }

    #[test]
    fn postgres_object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(PostgresBooleanConverter);
    }

    #[test]
    #[should_panic(expected = "expected boolean")]
    fn postgres_from_panics_on_number() {
        PostgresBooleanConverter.from(&Value::from(1));
    }

    #[test]
    #[should_panic(expected = "expected boolean")]
    fn postgres_to_panics_on_string() {
        PostgresBooleanConverter.to(&Value::from("true"));
    }
}
