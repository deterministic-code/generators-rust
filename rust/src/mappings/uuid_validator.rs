use serde_json::Value;

pub(crate) fn validate_uuid(datasource: &str, direction: &'static str, value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::String(s) if is_uuid_shape(s) => Value::String(s.clone()),
        Value::String(s) => panic!(
            "uuidConverter[{}].{} expected RFC-4122 UUID string, got '{}'",
            datasource, direction, s
        ),
        other => panic!(
            "uuidConverter[{}].{} expected UUID string, got {}",
            datasource, direction, other
        ),
    }
}

fn is_uuid_shape(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *b != b'-' {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_lowercase_uuid() {
        let v = Value::from("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(validate_uuid("sqlite", "to", &v), v);
    }

    #[test]
    fn accepts_uppercase_uuid() {
        let v = Value::from("550E8400-E29B-41D4-A716-446655440000");
        assert_eq!(validate_uuid("sqlite", "from", &v), v);
    }

    #[test]
    fn accepts_null() {
        assert!(validate_uuid("sqlite", "to", &Value::Null).is_null());
    }

    #[test]
    #[should_panic(expected = "expected RFC-4122 UUID string")]
    fn rejects_wrong_length() {
        validate_uuid("sqlite", "to", &Value::from("abc"));
    }

    #[test]
    #[should_panic(expected = "expected RFC-4122 UUID string")]
    fn rejects_missing_dashes() {
        validate_uuid(
            "sqlite",
            "to",
            &Value::from("550e8400e29b41d4a716446655440000"),
        );
    }

    #[test]
    #[should_panic(expected = "expected RFC-4122 UUID string")]
    fn rejects_non_hex_characters() {
        validate_uuid(
            "sqlite",
            "from",
            &Value::from("zzzzzzzz-e29b-41d4-a716-446655440000"),
        );
    }

    #[test]
    #[should_panic(expected = "expected UUID string")]
    fn rejects_non_string() {
        validate_uuid("sqlite", "to", &Value::from(42));
    }
}
