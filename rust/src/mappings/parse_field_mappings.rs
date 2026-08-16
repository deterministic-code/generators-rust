use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::RepositoryError;

pub type EntityFieldMap = BTreeMap<String, String>;
pub type FieldMappings = BTreeMap<String, EntityFieldMap>;

pub fn parse_field_mappings(datasource_data: &Value) -> Result<FieldMappings, RepositoryError> {
    if datasource_data.is_null() {
        return Ok(FieldMappings::new());
    }
    let root = datasource_data.as_object().ok_or_else(|| {
        RepositoryError::Other(format!(
            "parse_field_mappings: expected an object, got {}",
            kind(datasource_data),
        ))
    })?;

    let raw_mappings = match root.get("datasource_mappings") {
        None | Some(Value::Null) => return Ok(FieldMappings::new()),
        Some(v) => v,
    };
    let mappings_array = raw_mappings.as_array().ok_or_else(|| {
        RepositoryError::Other(format!(
            "parse_field_mappings: datasource_mappings must be an array, got {}",
            kind(raw_mappings),
        ))
    })?;

    let mut result: FieldMappings = FieldMappings::new();

    for (i, entry) in mappings_array.iter().enumerate() {
        let entry_obj = entry.as_object().ok_or_else(|| {
            RepositoryError::Other(format!(
                "parse_field_mappings: datasource_mappings[{}] must be an object, got {}",
                i,
                kind(entry),
            ))
        })?;
        if entry_obj.len() != 1 {
            return Err(RepositoryError::Other(format!(
                "parse_field_mappings: datasource_mappings[{}] must wrap exactly one entity name, got {} keys: {:?}",
                i,
                entry_obj.len(),
                entry_obj.keys().collect::<Vec<_>>(),
            )));
        }
        let entity_name = entry_obj.keys().next().expect("entity name").clone();
        let entity_body = entry_obj.get(&entity_name).expect("entity body");
        let entity_body_obj = entity_body.as_object().ok_or_else(|| {
            RepositoryError::Other(format!(
                "parse_field_mappings: datasource_mappings[{}].{} must be an object, got {}",
                i,
                entity_name,
                kind(entity_body),
            ))
        })?;

        let raw_field_mappings = match entity_body_obj.get("field_mappings") {
            None | Some(Value::Null) => continue,
            Some(v) => v,
        };
        let field_mappings_array = raw_field_mappings.as_array().ok_or_else(|| {
            RepositoryError::Other(format!(
                "parse_field_mappings: datasource_mappings[{}].{}.field_mappings must be an array, got {}",
                i,
                entity_name,
                kind(raw_field_mappings),
            ))
        })?;

        let mut entity_map = EntityFieldMap::new();
        for (j, fm) in field_mappings_array.iter().enumerate() {
            let fm_obj = fm.as_object().ok_or_else(|| {
                RepositoryError::Other(format!(
                    "parse_field_mappings: datasource_mappings[{}].{}.field_mappings[{}] must be an object, got {}",
                    i, entity_name, j, kind(fm),
                ))
            })?;
            if fm_obj.len() != 1 {
                return Err(RepositoryError::Other(format!(
                    "parse_field_mappings: datasource_mappings[{}].{}.field_mappings[{}] must wrap exactly one column name, got {} keys: {:?}",
                    i, entity_name, j, fm_obj.len(), fm_obj.keys().collect::<Vec<_>>(),
                )));
            }
            let logical_col = fm_obj.keys().next().expect("logical col").clone();
            let col_body = fm_obj.get(&logical_col).expect("col body");
            let col_body_obj = col_body.as_object().ok_or_else(|| {
                RepositoryError::Other(format!(
                    "parse_field_mappings: datasource_mappings[{}].{}.field_mappings[{}].{} must be an object, got {}",
                    i, entity_name, j, logical_col, kind(col_body),
                ))
            })?;
            let source = col_body_obj.get("source").ok_or_else(|| {
                RepositoryError::Other(format!(
                    "parse_field_mappings: datasource_mappings[{}].{}.field_mappings[{}].{}.source must be a string, got missing",
                    i, entity_name, j, logical_col,
                ))
            })?;
            let source_str = source.as_str().ok_or_else(|| {
                RepositoryError::Other(format!(
                    "parse_field_mappings: datasource_mappings[{}].{}.field_mappings[{}].{}.source must be a string, got {}: {}",
                    i, entity_name, j, logical_col, kind(source), source,
                ))
            })?;
            if entity_map.contains_key(&logical_col) {
                return Err(RepositoryError::Other(format!(
                    "parse_field_mappings: datasource_mappings[{}].{} declares column '{}' twice in field_mappings",
                    i, entity_name, logical_col,
                )));
            }
            entity_map.insert(logical_col, source_str.to_string());
        }
        if !entity_map.is_empty() {
            let existing = result.entry(entity_name).or_default();
            for (k, v) in entity_map {
                existing.insert(k, v);
            }
        }
    }

    Ok(result)
}

pub fn get_entity_field_map(mappings: &FieldMappings, entity_name: &str) -> EntityFieldMap {
    mappings.get(entity_name).cloned().unwrap_or_default()
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn returns_empty_when_input_is_null() {
        let out = parse_field_mappings(&Value::Null).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn returns_empty_when_datasource_mappings_key_is_missing() {
        let out = parse_field_mappings(&json!({})).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn returns_empty_when_datasource_mappings_is_null() {
        let out = parse_field_mappings(&json!({ "datasource_mappings": null })).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn parses_happy_path_single_entity_single_field() {
        let input = json!({
            "datasource_mappings": [
                {
                    "user": {
                        "field_mappings": [
                            { "username": { "source": "user_name" } }
                        ]
                    }
                }
            ]
        });
        let out = parse_field_mappings(&input).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            out.get("user").unwrap().get("username").unwrap(),
            "user_name"
        );
    }

    #[test]
    fn parses_multiple_columns_for_same_entity() {
        let input = json!({
            "datasource_mappings": [
                {
                    "user": {
                        "field_mappings": [
                            { "first_name": { "source": "fn" } },
                            { "last_name": { "source": "ln" } }
                        ]
                    }
                }
            ]
        });
        let out = parse_field_mappings(&input).unwrap();
        let user = out.get("user").unwrap();
        assert_eq!(user.len(), 2);
        assert_eq!(user.get("first_name").unwrap(), "fn");
        assert_eq!(user.get("last_name").unwrap(), "ln");
    }

    #[test]
    fn merges_multiple_entries_for_same_entity_name() {
        let input = json!({
            "datasource_mappings": [
                { "user": { "field_mappings": [ { "a": { "source": "x" } } ] } },
                { "user": { "field_mappings": [ { "b": { "source": "y" } } ] } }
            ]
        });
        let out = parse_field_mappings(&input).unwrap();
        let user = out.get("user").unwrap();
        assert_eq!(user.len(), 2);
        assert_eq!(user.get("a").unwrap(), "x");
        assert_eq!(user.get("b").unwrap(), "y");
    }

    #[test]
    fn throws_when_datasource_data_is_not_object() {
        let err = parse_field_mappings(&json!("hello")).unwrap_err();
        assert!(err.to_string().contains("expected an object"));
    }

    #[test]
    fn throws_when_datasource_mappings_is_not_array() {
        let err = parse_field_mappings(&json!({ "datasource_mappings": "oops" })).unwrap_err();
        assert!(err.to_string().contains("must be an array"));
    }

    #[test]
    fn throws_when_entry_is_not_object() {
        let err = parse_field_mappings(&json!({ "datasource_mappings": ["string"] })).unwrap_err();
        assert!(err.to_string().contains("must be an object"));
    }

    #[test]
    fn throws_when_entry_has_zero_or_multiple_keys() {
        let err = parse_field_mappings(&json!({
            "datasource_mappings": [ { "a": {}, "b": {} } ]
        }))
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("must wrap exactly one entity name"));
    }

    #[test]
    fn throws_when_field_mappings_is_not_array() {
        let err = parse_field_mappings(&json!({
            "datasource_mappings": [ { "user": { "field_mappings": "oops" } } ]
        }))
        .unwrap_err();
        assert!(err.to_string().contains("must be an array"));
    }

    #[test]
    fn throws_when_field_mapping_entry_has_zero_or_multiple_keys() {
        let err = parse_field_mappings(&json!({
            "datasource_mappings": [
                { "user": { "field_mappings": [ { "a": { "source": "x" }, "b": { "source": "y" } } ] } }
            ]
        })).unwrap_err();
        assert!(err
            .to_string()
            .contains("must wrap exactly one column name"));
    }

    #[test]
    fn throws_when_source_is_missing_or_not_string() {
        let err = parse_field_mappings(&json!({
            "datasource_mappings": [
                { "user": { "field_mappings": [ { "username": { "scource": "typo" } } ] } }
            ]
        }))
        .unwrap_err();
        assert!(
            err.to_string().contains("source must be a string"),
            "{}",
            err
        );

        let err2 = parse_field_mappings(&json!({
            "datasource_mappings": [
                { "user": { "field_mappings": [ { "username": { "source": 42 } } ] } }
            ]
        }))
        .unwrap_err();
        assert!(
            err2.to_string().contains("source must be a string"),
            "{}",
            err2
        );
    }

    #[test]
    fn throws_when_same_logical_column_declared_twice_for_one_entity() {
        let err = parse_field_mappings(&json!({
            "datasource_mappings": [
                { "user": { "field_mappings": [
                    { "username": { "source": "u1" } },
                    { "username": { "source": "u2" } }
                ] } }
            ]
        }))
        .unwrap_err();
        assert!(err.to_string().contains("twice"), "{}", err);
    }

    #[test]
    fn skips_entity_with_no_field_mappings_key_without_creating_empty_entry() {
        let input = json!({
            "datasource_mappings": [
                { "user": { "field_mappings": null } },
                { "user": {} }
            ]
        });
        let out = parse_field_mappings(&input).unwrap();
        assert!(out.is_empty(), "no field mappings → no entry");
    }

    #[test]
    fn get_entity_field_map_returns_empty_for_unknown_entity() {
        let mappings = FieldMappings::new();
        let out = get_entity_field_map(&mappings, "nope");
        assert!(out.is_empty());
    }
}
