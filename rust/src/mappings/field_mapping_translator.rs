use std::collections::BTreeMap;

use crate::error::RepositoryError;
use crate::mappings::parse_field_mappings::EntityFieldMap;
use crate::repositories::row_map::RowMap;

#[derive(Debug)]
pub struct FieldMappingTranslator {
    logical_to_physical: BTreeMap<String, String>,
    physical_to_logical: BTreeMap<String, String>,
    has_mappings: bool,
}

impl FieldMappingTranslator {
    pub fn new(entity_map: Option<&EntityFieldMap>) -> Result<Self, RepositoryError> {
        let Some(map) = entity_map.filter(|m| !m.is_empty()) else {
            return Ok(Self {
                logical_to_physical: BTreeMap::new(),
                physical_to_logical: BTreeMap::new(),
                has_mappings: false,
            });
        };
        let mut reverse: BTreeMap<String, String> = BTreeMap::new();
        for (logical, physical) in map {
            if let Some(existing) = reverse.get(physical) {
                return Err(RepositoryError::Other(format!(
                    "FieldMappingTranslator: physical column '{}' is mapped from multiple logical columns ('{}' and '{}')",
                    physical, existing, logical,
                )));
            }
            reverse.insert(physical.clone(), logical.clone());
        }
        Ok(Self {
            logical_to_physical: map.clone(),
            physical_to_logical: reverse,
            has_mappings: true,
        })
    }

    pub fn has_mappings(&self) -> bool {
        self.has_mappings
    }

    pub fn to_physical(&self, logical: &str) -> String {
        self.logical_to_physical
            .get(logical)
            .cloned()
            .unwrap_or_else(|| logical.to_string())
    }

    pub fn to_logical(&self, physical: &str) -> String {
        self.physical_to_logical
            .get(physical)
            .cloned()
            .unwrap_or_else(|| physical.to_string())
    }

    pub fn to_physical_row(&self, row: RowMap) -> RowMap {
        if !self.has_mappings {
            return row;
        }
        let mut out = RowMap::new();
        for (k, v) in row {
            out.insert(self.to_physical(&k), v);
        }
        out
    }

    pub fn to_logical_row(&self, row: RowMap) -> RowMap {
        if !self.has_mappings {
            return row;
        }
        let mut out = RowMap::new();
        for (k, v) in row {
            out.insert(self.to_logical(&k), v);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn map_of(pairs: &[(&str, &str)]) -> EntityFieldMap {
        let mut m = EntityFieldMap::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), (*v).to_string());
        }
        m
    }

    #[test]
    fn empty_input_produces_passthrough_translator_with_no_mappings_flag() {
        let t = FieldMappingTranslator::new(None).unwrap();
        assert!(!t.has_mappings());
        assert_eq!(t.to_physical("anything"), "anything");
        assert_eq!(t.to_logical("anything"), "anything");
    }

    #[test]
    fn empty_map_input_also_produces_passthrough() {
        let m = EntityFieldMap::new();
        let t = FieldMappingTranslator::new(Some(&m)).unwrap();
        assert!(!t.has_mappings());
    }

    #[test]
    fn to_physical_returns_mapped_or_passthrough() {
        let m = map_of(&[("username", "user_name")]);
        let t = FieldMappingTranslator::new(Some(&m)).unwrap();
        assert!(t.has_mappings());
        assert_eq!(t.to_physical("username"), "user_name");
        assert_eq!(t.to_physical("id"), "id");
    }

    #[test]
    fn to_logical_returns_mapped_or_passthrough() {
        let m = map_of(&[("username", "user_name")]);
        let t = FieldMappingTranslator::new(Some(&m)).unwrap();
        assert_eq!(t.to_logical("user_name"), "username");
        assert_eq!(t.to_logical("id"), "id");
    }

    #[test]
    fn roundtrip_logical_to_physical_to_logical() {
        let m = map_of(&[("username", "user_name"), ("first_name", "fn")]);
        let t = FieldMappingTranslator::new(Some(&m)).unwrap();
        for logical in &["username", "first_name", "unmapped"] {
            assert_eq!(t.to_logical(&t.to_physical(logical)), *logical);
        }
    }

    #[test]
    fn to_physical_row_renames_mapped_keys_only() {
        let m = map_of(&[("username", "user_name")]);
        let t = FieldMappingTranslator::new(Some(&m)).unwrap();
        let mut row = RowMap::new();
        row.insert("id".to_string(), Value::from(1));
        row.insert("username".to_string(), Value::from("alice"));
        let physical = t.to_physical_row(row);
        assert_eq!(physical.get("id").unwrap().as_i64(), Some(1));
        assert_eq!(physical.get("user_name").unwrap().as_str(), Some("alice"));
        assert!(physical.get("username").is_none());
    }

    #[test]
    fn to_logical_row_renames_mapped_keys_only() {
        let m = map_of(&[("username", "user_name")]);
        let t = FieldMappingTranslator::new(Some(&m)).unwrap();
        let mut row = RowMap::new();
        row.insert("id".to_string(), Value::from(1));
        row.insert("user_name".to_string(), Value::from("alice"));
        let logical = t.to_logical_row(row);
        assert_eq!(logical.get("username").unwrap().as_str(), Some("alice"));
        assert!(logical.get("user_name").is_none());
    }

    #[test]
    fn passthrough_translator_returns_row_unchanged() {
        let t = FieldMappingTranslator::new(None).unwrap();
        let mut row = RowMap::new();
        row.insert("a".to_string(), Value::from(1));
        let out = t.to_physical_row(row);
        assert_eq!(out.get("a").unwrap().as_i64(), Some(1));
    }

    #[test]
    fn throws_when_two_logical_columns_map_to_same_physical_column() {
        let m = map_of(&[("a", "x"), ("b", "x")]);
        let err = FieldMappingTranslator::new(Some(&m)).unwrap_err();
        assert!(err
            .to_string()
            .contains("mapped from multiple logical columns"));
    }
}
