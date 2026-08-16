use serde_json::Value;

use crate::repositories::datasource_middleware::DatabaseBackend;

pub trait TypeFieldConverter: Send + Sync {
    fn from_datasource(&self) -> DatabaseBackend;
    fn datasource_type(&self) -> &str;
    fn from(&self, value: &Value) -> Value;
    fn to(&self, value: &Value) -> Value;
    // why postgres only: sqlx binds Value::String as text; PG won't implicitly cast text→timestamptz/uuid/etc., so a converter declares the SQL fragment to append to its bound placeholder.
    fn bind_sql_cast(&self) -> Option<&str> {
        None
    }
}

/// Shared `from` for every dialect's binary converter: the BLOB/BYTEA decoders yield the
/// stored bytes as a JSON number array, but binary is carried on the wire as a string
/// (OpenAPI `format: byte`), so reassemble that string from the byte array. A null or a
/// value that is already a string passes through unchanged.
pub fn binary_bytes_to_string(value: &Value) -> Value {
    let Value::Array(items) = value else {
        return value.clone();
    };
    let mut bytes = Vec::with_capacity(items.len());
    for item in items {
        match item.as_u64() {
            Some(n) if n <= u8::MAX as u64 => bytes.push(n as u8),
            _ => return value.clone(),
        }
    }
    Value::String(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct UppercaseStringConverter;

    impl TypeFieldConverter for UppercaseStringConverter {
        fn from_datasource(&self) -> DatabaseBackend {
            DatabaseBackend::Sqlite
        }
        fn datasource_type(&self) -> &str {
            "string"
        }
        fn from(&self, value: &Value) -> Value {
            match value.as_str() {
                Some(s) => Value::from(s.to_uppercase()),
                None => value.clone(),
            }
        }
        fn to(&self, value: &Value) -> Value {
            match value.as_str() {
                Some(s) => Value::from(s.to_lowercase()),
                None => value.clone(),
            }
        }
    }

    #[test]
    fn converter_advertises_source_and_type() {
        let c = UppercaseStringConverter;
        assert_eq!(c.from_datasource(), DatabaseBackend::Sqlite);
        assert_eq!(c.datasource_type(), "string");
    }

    #[test]
    fn from_db_to_language_roundtrip() {
        let c = UppercaseStringConverter;
        let from = c.from(&Value::from("alice"));
        assert_eq!(from.as_str(), Some("ALICE"));
        let back = c.to(&from);
        assert_eq!(back.as_str(), Some("alice"));
    }

    #[test]
    fn null_passes_through_unchanged() {
        let c = UppercaseStringConverter;
        assert!(c.from(&Value::Null).is_null());
        assert!(c.to(&Value::Null).is_null());
    }

    #[test]
    fn trait_is_object_safe_via_arc_dyn() {
        let _c: Arc<dyn TypeFieldConverter> = Arc::new(UppercaseStringConverter);
    }

    #[test]
    fn binary_bytes_to_string_reassembles_wire_string() {
        let bytes = Value::Array(vec![
            Value::from(104u8),
            Value::from(105u8),
            Value::from(33u8),
        ]);
        assert_eq!(binary_bytes_to_string(&bytes), Value::from("hi!"));
    }

    #[test]
    fn binary_bytes_to_string_passes_through_non_array() {
        assert_eq!(binary_bytes_to_string(&Value::Null), Value::Null);
        assert_eq!(
            binary_bytes_to_string(&Value::from("Y3J1ZA==")),
            Value::from("Y3J1ZA==")
        );
    }

    #[test]
    fn binary_bytes_to_string_leaves_out_of_range_array_untouched() {
        let arr = Value::Array(vec![Value::from(300u64)]);
        assert_eq!(binary_bytes_to_string(&arr), arr);
    }
}
