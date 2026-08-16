use std::path::Path;

use serde::Deserialize;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum FieldSize {
    Bounded(u32),
    Unlimited,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FieldDef {
    pub name: String,
    pub r#type: String,
    pub size: Option<FieldSize>,
    pub primary_key: bool,
    pub is_nullable: bool,
    pub is_unique: bool,
    pub references: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DatasourceTypeDef {
    pub name: String,
    pub datasource_type: Option<String>,
    pub skip_migrations: bool,
    pub optimistic_concurrency: Option<bool>,
    pub target: Option<String>,
    pub fields: Vec<FieldDef>,
    pub seeds: Vec<SeedRow>,
}

impl DatasourceTypeDef {
    /// Whether this entity participates in optimistic concurrency: junction and readonly-lookup
    /// tables never do, an explicit per-type `use_optimistic_concurrency` overrides the
    /// datasource-wide flag, and everything else inherits it. Mirrors the codegen resolver
    /// (scripts/lib/emit-sql.ts `entityUsesOptimisticConcurrency`) so the emitted router and this
    /// runtime service agree per entity.
    pub fn uses_optimistic_concurrency(&self, global_flag: bool) -> bool {
        match self.datasource_type.as_deref() {
            Some("many-to-many") | Some("readonly-lookup") => false,
            _ => self.optimistic_concurrency.unwrap_or(global_flag),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeedRow {
    pub id: i64,
    pub values: Vec<(String, JsonValue)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FieldMapping {
    pub field: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DatasourceMapping {
    pub entity: String,
    pub source: String,
    pub field_mappings: Vec<FieldMapping>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DatasourceTypesDoc {
    pub types: Vec<DatasourceTypeDef>,
    pub datasource_mappings: Vec<DatasourceMapping>,
}

#[derive(Debug, Error)]
pub enum DatasourceTypesError {
    #[error("io error reading datasource_types: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("datasource_types.yaml: {0}")]
    Schema(String),
}

#[derive(Deserialize, Default)]
struct RawDoc {
    #[serde(default)]
    types: Vec<YamlValue>,
    #[serde(default)]
    datasource_mappings: Vec<YamlValue>,
}

#[derive(Deserialize, Default)]
struct RawFieldFlags {
    #[serde(rename = "type")]
    type_: Option<String>,
    size: Option<YamlValue>,
    primary_key: Option<bool>,
    is_nullable: Option<bool>,
    is_unique: Option<bool>,
    references: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawEntityBody {
    datasource_type: Option<String>,
    skip_migrations: Option<bool>,
    use_optimistic_concurrency: Option<bool>,
    target: Option<String>,
    #[serde(default)]
    fields: Vec<YamlValue>,
    #[serde(default)]
    seeds: Vec<YamlValue>,
}

#[derive(Deserialize, Default)]
struct RawMappingBody {
    source: String,
    #[serde(default)]
    field_mappings: Vec<YamlValue>,
}

pub fn parse_datasource_types(yaml: &str) -> Result<DatasourceTypesDoc, DatasourceTypesError> {
    if yaml.trim().is_empty() {
        return Ok(DatasourceTypesDoc::default());
    }
    let doc: RawDoc = serde_yaml::from_str(yaml)?;
    let mut out = DatasourceTypesDoc::default();
    for raw in doc.types {
        out.types.push(parse_entity(raw)?);
    }
    for raw in doc.datasource_mappings {
        out.datasource_mappings.push(parse_mapping(raw)?);
    }
    resolve_reference_types(&mut out)?;
    Ok(out)
}

pub fn load_datasource_types(path: &Path) -> Result<DatasourceTypesDoc, DatasourceTypesError> {
    if !path.exists() {
        return Ok(DatasourceTypesDoc::default());
    }
    let text = std::fs::read_to_string(path)?;
    parse_datasource_types(&text)
}

fn parse_entity(raw: YamlValue) -> Result<DatasourceTypeDef, DatasourceTypesError> {
    let (name, body) = take_single_keyed_map(raw, "datasource type entry")?;
    let parsed: RawEntityBody = serde_yaml::from_value(body)
        .map_err(|e| DatasourceTypesError::Schema(format!("entity '{}': {}", name, e)))?;
    let mut fields = Vec::with_capacity(parsed.fields.len());
    for raw_field in parsed.fields {
        fields.push(parse_field(raw_field, &name)?);
    }
    let mut seeds = Vec::with_capacity(parsed.seeds.len());
    for raw_seed in parsed.seeds {
        seeds.push(parse_seed(raw_seed, &name)?);
    }
    Ok(DatasourceTypeDef {
        name,
        datasource_type: parsed.datasource_type,
        skip_migrations: parsed.skip_migrations.unwrap_or(false),
        optimistic_concurrency: parsed.use_optimistic_concurrency,
        target: parsed.target,
        fields,
        seeds,
    })
}

fn parse_seed(raw: YamlValue, entity: &str) -> Result<SeedRow, DatasourceTypesError> {
    let (key, body) = take_single_keyed_map(raw, "seed entry")?;
    let id_digits = key.strip_prefix("id").ok_or_else(|| {
        DatasourceTypesError::Schema(format!(
            "entity '{}' seed key '{}': expected pattern /^id\\d+$/",
            entity, key
        ))
    })?;
    let id: i64 = id_digits.parse().map_err(|_| {
        DatasourceTypesError::Schema(format!(
            "entity '{}' seed key '{}': expected pattern /^id\\d+$/",
            entity, key
        ))
    })?;
    let values = match body {
        YamlValue::Mapping(map) => {
            let mut out = Vec::with_capacity(map.len());
            for (k, v) in map {
                let YamlValue::String(field_name) = k else {
                    return Err(DatasourceTypesError::Schema(format!(
                        "entity '{}' seed '{}': field key must be a string",
                        entity, key
                    )));
                };
                let json: JsonValue = serde_yaml::from_value(v).map_err(|e| {
                    DatasourceTypesError::Schema(format!(
                        "entity '{}' seed '{}' field '{}': {}",
                        entity, key, field_name, e
                    ))
                })?;
                out.push((field_name, json));
            }
            out
        }
        other => {
            return Err(DatasourceTypesError::Schema(format!(
                "entity '{}' seed '{}': body must be a mapping (got {:?})",
                entity, key, other
            )));
        }
    };
    Ok(SeedRow { id, values })
}

fn parse_field(raw: YamlValue, entity: &str) -> Result<FieldDef, DatasourceTypesError> {
    let (name, body) = take_single_keyed_map(raw, "field entry")?;
    let parsed: RawFieldFlags = serde_yaml::from_value(body).map_err(|e| {
        DatasourceTypesError::Schema(format!("entity '{}' field '{}': {}", entity, name, e))
    })?;
    let type_ = match parsed.type_ {
        Some(t) => t,
        // A foreign key may omit `type` (spec referenceField); it inherits the referenced
        // parent id's type. The `reference` sentinel matches how the removed `{type: reference}`
        // form persisted — i64 path-param coercion, converter passthrough.
        None if parsed.references.is_some() => "reference".to_string(),
        None => {
            return Err(DatasourceTypesError::Schema(format!(
                "entity '{}' field '{}': missing `type`",
                entity, name
            )));
        }
    };
    let size = match parsed.size {
        None | Some(YamlValue::Null) => None,
        Some(YamlValue::String(s)) if s == "unlimited" => Some(FieldSize::Unlimited),
        Some(YamlValue::String(s)) => {
            let n: u32 = s.parse().map_err(|_| {
                DatasourceTypesError::Schema(format!(
                    "entity '{}' field '{}': invalid size '{}'",
                    entity, name, s
                ))
            })?;
            Some(FieldSize::Bounded(n))
        }
        Some(YamlValue::Number(n)) => {
            let v = n.as_u64().ok_or_else(|| {
                DatasourceTypesError::Schema(format!(
                    "entity '{}' field '{}': size must be a positive integer",
                    entity, name
                ))
            })?;
            Some(FieldSize::Bounded(v as u32))
        }
        Some(other) => {
            return Err(DatasourceTypesError::Schema(format!(
                "entity '{}' field '{}': size must be a string or number (got {:?})",
                entity, name, other
            )));
        }
    };
    Ok(FieldDef {
        name,
        r#type: type_,
        size,
        primary_key: parsed.primary_key.unwrap_or(false),
        is_nullable: parsed.is_nullable.unwrap_or(false),
        is_unique: parsed.is_unique.unwrap_or(false),
        references: parsed.references,
    })
}

fn parse_mapping(raw: YamlValue) -> Result<DatasourceMapping, DatasourceTypesError> {
    let (entity, body) = take_single_keyed_map(raw, "datasource_mappings entry")?;
    let parsed: RawMappingBody = serde_yaml::from_value(body)
        .map_err(|e| DatasourceTypesError::Schema(format!("mapping '{}': {}", entity, e)))?;
    let mut mappings = Vec::with_capacity(parsed.field_mappings.len());
    for raw_field in parsed.field_mappings {
        let (field, body) = take_single_keyed_map(raw_field, "field_mappings entry")?;
        let source = match body {
            YamlValue::Mapping(map) => map
                .get(YamlValue::String("source".to_string()))
                .and_then(|v| match v {
                    YamlValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .ok_or_else(|| {
                    DatasourceTypesError::Schema(format!(
                        "mapping '{}' field '{}': missing string `source`",
                        entity, field
                    ))
                })?,
            other => {
                return Err(DatasourceTypesError::Schema(format!(
                    "mapping '{}' field '{}': body must be a mapping (got {:?})",
                    entity, field, other
                )));
            }
        };
        mappings.push(FieldMapping { field, source });
    }
    Ok(DatasourceMapping {
        entity,
        source: parsed.source,
        field_mappings: mappings,
    })
}

fn take_single_keyed_map(
    raw: YamlValue,
    context: &str,
) -> Result<(String, YamlValue), DatasourceTypesError> {
    match raw {
        YamlValue::Mapping(map) => {
            if map.len() != 1 {
                return Err(DatasourceTypesError::Schema(format!(
                    "{}: expected single-key map (got {} keys)",
                    context,
                    map.len()
                )));
            }
            let (k, v) = map.into_iter().next().unwrap();
            let name = match k {
                YamlValue::String(s) => s,
                _ => {
                    return Err(DatasourceTypesError::Schema(format!(
                        "{}: key must be a string",
                        context
                    )));
                }
            };
            Ok((name, v))
        }
        other => Err(DatasourceTypesError::Schema(format!(
            "{}: expected mapping (got {:?})",
            context, other
        ))),
    }
}

/// A typeless FK (spec `referenceField`) parses with the sentinel `reference` type; it inherits the
/// referenced parent's declared primary-key type. Two passes avoid borrowing `doc.types` mutably and
/// immutably at once: first collect each entity's explicit-PK type, then rewrite the `reference` fields.
/// `references` is `<entity>.<column>`, so the parent name is the segment before the first `.`.
fn resolve_reference_types(doc: &mut DatasourceTypesDoc) -> Result<(), DatasourceTypesError> {
    let pk_types: std::collections::HashMap<String, String> = doc
        .types
        .iter()
        .filter_map(|e| {
            e.fields
                .iter()
                .find(|f| f.primary_key)
                .map(|f| (e.name.clone(), f.r#type.clone()))
        })
        .collect();
    for entity in &mut doc.types {
        for field in &mut entity.fields {
            if field.r#type != "reference" {
                continue;
            }
            let Some(references) = &field.references else {
                continue;
            };
            let parent = references.split('.').next().unwrap_or(references);
            if let Some(parent_pk_type) = pk_types.get(parent) {
                field.r#type = parent_pk_type.clone();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_yaml_returns_empty_doc() {
        let doc = parse_datasource_types("").unwrap();
        assert!(doc.types.is_empty());
        assert!(doc.datasource_mappings.is_empty());
    }

    fn def_with(datasource_type: Option<&str>, occ: Option<bool>) -> DatasourceTypeDef {
        DatasourceTypeDef {
            name: "e".to_string(),
            datasource_type: datasource_type.map(str::to_string),
            optimistic_concurrency: occ,
            ..Default::default()
        }
    }

    #[test]
    fn uses_optimistic_concurrency_inherits_global_without_override() {
        assert!(def_with(None, None).uses_optimistic_concurrency(true));
        assert!(!def_with(None, None).uses_optimistic_concurrency(false));
    }

    #[test]
    fn uses_optimistic_concurrency_per_type_override_wins() {
        assert!(!def_with(None, Some(false)).uses_optimistic_concurrency(true));
        assert!(def_with(None, Some(true)).uses_optimistic_concurrency(false));
    }

    #[test]
    fn uses_optimistic_concurrency_junction_and_lookup_never_participate() {
        for kind in ["many-to-many", "readonly-lookup"] {
            assert!(!def_with(Some(kind), Some(true)).uses_optimistic_concurrency(true));
        }
    }

    #[test]
    fn parses_per_type_use_optimistic_concurrency() {
        let yaml = concat!(
            "types:\n",
            "  - legacy:\n",
            "      use_optimistic_concurrency: false\n",
            "      fields:\n",
            "        - key:\n",
            "            type: string\n",
            "            primary_key: true\n",
        );
        let doc = parse_datasource_types(yaml).unwrap();
        assert_eq!(doc.types[0].optimistic_concurrency, Some(false));
    }

    #[test]
    fn parses_minimal_entity() {
        let yaml = concat!(
            "types:\n",
            "  - widget:\n",
            "      target: Crud\n",
            "      fields:\n",
            "        - id:\n",
            "            type: integer\n",
            "            primary_key: true\n",
        );
        let doc = parse_datasource_types(yaml).unwrap();
        assert_eq!(doc.types.len(), 1);
        let e = &doc.types[0];
        assert_eq!(e.name, "widget");
        assert_eq!(e.target.as_deref(), Some("Crud"));
        assert_eq!(e.fields.len(), 1);
        assert_eq!(e.fields[0].name, "id");
        assert_eq!(e.fields[0].r#type, "integer");
        assert!(e.fields[0].primary_key);
    }

    #[test]
    fn parses_readonly_lookup_with_size_and_skip_migrations() {
        let yaml = concat!(
            "types:\n",
            "  - notification_type:\n",
            "      datasource_type: \"readonly-lookup\"\n",
            "      skip_migrations: true\n",
            "      target: Crud\n",
            "      fields:\n",
            "        - name:\n",
            "            type: string\n",
            "            size: 256\n",
            "            primary_key: true\n",
            "        - description:\n",
            "            type: string\n",
            "            size: unlimited\n",
            "            is_nullable: true\n",
        );
        let doc = parse_datasource_types(yaml).unwrap();
        let e = &doc.types[0];
        assert_eq!(e.datasource_type.as_deref(), Some("readonly-lookup"));
        assert!(e.skip_migrations);
        assert_eq!(e.fields.len(), 2);
        assert_eq!(e.fields[0].size, Some(FieldSize::Bounded(256)));
        assert_eq!(e.fields[1].size, Some(FieldSize::Unlimited));
        assert!(e.fields[1].is_nullable);
    }

    #[test]
    fn parses_references() {
        let yaml = concat!(
            "types:\n",
            "  - notification:\n",
            "      target: Crud\n",
            "      fields:\n",
            "        - notification_type:\n",
            "            type: string\n",
            "            size: 256\n",
            "            references: notification_type.name\n",
        );
        let doc = parse_datasource_types(yaml).unwrap();
        assert_eq!(
            doc.types[0].fields[0].references.as_deref(),
            Some("notification_type.name")
        );
    }

    #[test]
    fn typeless_reference_field_inherits_reference_type() {
        let yaml = concat!(
            "types:\n",
            "  - application_user:\n",
            "      datasource_type: \"many-to-many\"\n",
            "      fields:\n",
            "        - application_id:\n",
            "            type: number\n",
            "            references: application.id\n",
            "        - user_id:\n",
            "            references: user.id\n",
        );
        let doc = parse_datasource_types(yaml).unwrap();
        let fields = &doc.types[0].fields;
        assert_eq!(fields[0].r#type, "number");
        assert_eq!(fields[1].name, "user_id");
        assert_eq!(fields[1].r#type, "reference");
        assert_eq!(fields[1].references.as_deref(), Some("user.id"));
    }

    #[test]
    fn parses_datasource_mappings() {
        let yaml = concat!(
            "types: []\n",
            "datasource_mappings:\n",
            "  - notification_type:\n",
            "      source: notification_types\n",
            "      field_mappings:\n",
            "        - name:\n",
            "            source: name2\n",
            "        - description:\n",
            "            source: _description_text\n",
        );
        let doc = parse_datasource_types(yaml).unwrap();
        assert_eq!(doc.datasource_mappings.len(), 1);
        let m = &doc.datasource_mappings[0];
        assert_eq!(m.entity, "notification_type");
        assert_eq!(m.source, "notification_types");
        assert_eq!(m.field_mappings.len(), 2);
        assert_eq!(m.field_mappings[0].field, "name");
        assert_eq!(m.field_mappings[0].source, "name2");
    }

    #[test]
    fn missing_field_type_errors() {
        let yaml = concat!(
            "types:\n",
            "  - widget:\n",
            "      fields:\n",
            "        - id:\n",
            "            primary_key: true\n",
        );
        let err = parse_datasource_types(yaml).unwrap_err();
        assert!(format!("{}", err).contains("missing `type`"));
    }

    #[test]
    fn parses_seeds_with_string_keys_and_typed_values() {
        let yaml = concat!(
            "types:\n",
            "  - contact_source:\n",
            "      datasource_type: \"readonly-lookup\"\n",
            "      fields:\n",
            "        - name:\n",
            "            type: string\n",
            "            size: 64\n",
            "            is_unique: true\n",
            "        - description:\n",
            "            type: string\n",
            "            size: unlimited\n",
            "            is_nullable: true\n",
            "      seeds:\n",
            "        - id1:\n",
            "            name: Manual\n",
            "        - id2:\n",
            "            name: Imported\n",
            "            description: Loaded from a CSV or vCard import.\n",
            "        - id3:\n",
            "            name: OAuth\n",
            "            description: Synced from a third-party provider.\n",
        );
        let doc = parse_datasource_types(yaml).unwrap();
        let e = &doc.types[0];
        assert_eq!(e.seeds.len(), 3);
        assert_eq!(e.seeds[0].id, 1);
        assert_eq!(
            e.seeds[0].values,
            vec![("name".to_string(), JsonValue::from("Manual"))]
        );
        assert_eq!(e.seeds[1].id, 2);
        assert_eq!(e.seeds[1].values.len(), 2);
        assert_eq!(e.seeds[2].id, 3);
    }

    #[test]
    fn invalid_seed_key_errors() {
        let yaml = concat!(
            "types:\n",
            "  - widget:\n",
            "      fields:\n",
            "        - name:\n",
            "            type: string\n",
            "            size: 16\n",
            "      seeds:\n",
            "        - first:\n",
            "            name: foo\n",
        );
        let err = parse_datasource_types(yaml).unwrap_err();
        assert!(format!("{}", err).contains("expected pattern"));
    }

    #[test]
    fn target_none_is_parsed() {
        let yaml = concat!(
            "types:\n",
            "  - event_log:\n",
            "      target: None\n",
            "      fields:\n",
            "        - occurred_at:\n",
            "            type: datetime\n",
        );
        let doc = parse_datasource_types(yaml).unwrap();
        assert_eq!(doc.types[0].target.as_deref(), Some("None"));
    }
}
