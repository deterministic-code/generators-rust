use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DatasourceInclusion {
    pub include: Option<String>,
    pub filter: Option<String>,
    pub auto_enrich: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ViewFieldDef {
    pub name: String,
    pub r#type: String,
    pub size: Option<String>,
    pub references: Option<String>,
    pub is_nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ViewTypeDef {
    pub name: String,
    pub inherits: Option<String>,
    pub fields: Vec<ViewFieldDef>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ViewTypesDoc {
    pub datasource_inclusion: DatasourceInclusion,
    pub types: Vec<ViewTypeDef>,
}

#[derive(Debug, Error)]
pub enum ViewTypesError {
    #[error("io error reading view_types: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("view_types.yaml: {0}")]
    Schema(String),
}

#[derive(Deserialize, Default)]
struct RawDoc {
    #[serde(default)]
    datasource_types: Option<RawInclusion>,
    #[serde(default)]
    includes: Vec<YamlValue>,
    #[serde(default)]
    types: Vec<YamlValue>,
}

#[derive(Deserialize, Default)]
struct RawInclusion {
    include: Option<String>,
    filter: Option<String>,
    auto_enrich: Option<bool>,
}

#[derive(Deserialize, Default)]
struct RawBody {
    inherits: Option<String>,
    #[serde(default)]
    fields: Vec<YamlValue>,
}

#[derive(Deserialize, Default)]
struct RawFieldFlags {
    #[serde(rename = "type")]
    type_: Option<String>,
    size: Option<YamlValue>,
    references: Option<String>,
    is_nullable: Option<bool>,
}

fn inclusion_from_includes(includes: &[YamlValue]) -> Option<RawInclusion> {
    for item in includes {
        if let Some(ds) = item.as_mapping().and_then(|m| m.get("datasource_types")) {
            return serde_yaml::from_value::<RawInclusion>(ds.clone()).ok();
        }
    }
    None
}

pub fn parse_view_types(yaml: &str) -> Result<ViewTypesDoc, ViewTypesError> {
    if yaml.trim().is_empty() {
        return Ok(ViewTypesDoc::default());
    }
    let doc: RawDoc = serde_yaml::from_str(yaml)?;
    // The datasource inclusion (carrying auto_enrich) is authored under the `includes:` directive the emitters emit; a bare top-level `datasource_types:` is the legacy shape. Reading only the latter silently defaulted auto_enrich to false, so enriched responses kept the raw FK id while the OpenAPI schema (which honors includes) omitted it.
    let raw_inclusion = doc
        .datasource_types
        .or_else(|| inclusion_from_includes(&doc.includes));
    let inclusion = match raw_inclusion {
        None => DatasourceInclusion::default(),
        Some(raw) => DatasourceInclusion {
            include: raw.include,
            filter: raw.filter,
            auto_enrich: raw.auto_enrich.unwrap_or(false),
        },
    };
    let mut types = Vec::with_capacity(doc.types.len());
    for raw in doc.types {
        types.push(parse_view(raw)?);
    }
    Ok(ViewTypesDoc {
        datasource_inclusion: inclusion,
        types,
    })
}

pub fn load_view_types(path: &Path) -> Result<ViewTypesDoc, ViewTypesError> {
    if !path.exists() {
        return Ok(ViewTypesDoc::default());
    }
    let text = std::fs::read_to_string(path)?;
    parse_view_types(&text)
}

fn parse_view(raw: YamlValue) -> Result<ViewTypeDef, ViewTypesError> {
    let (name, body) = take_single_keyed_map(raw, "view type entry")?;
    let parsed: RawBody = serde_yaml::from_value(body)
        .map_err(|e| ViewTypesError::Schema(format!("view '{}': {}", name, e)))?;
    let mut fields = Vec::with_capacity(parsed.fields.len());
    for raw_field in parsed.fields {
        fields.push(parse_field(raw_field, &name)?);
    }
    Ok(ViewTypeDef {
        name,
        inherits: parsed.inherits,
        fields,
    })
}

fn parse_field(raw: YamlValue, view: &str) -> Result<ViewFieldDef, ViewTypesError> {
    let (name, body) = take_single_keyed_map(raw, "view field entry")?;
    let parsed: RawFieldFlags = serde_yaml::from_value(body)
        .map_err(|e| ViewTypesError::Schema(format!("view '{}' field '{}': {}", view, name, e)))?;
    let type_ = parsed.type_.ok_or_else(|| {
        ViewTypesError::Schema(format!("view '{}' field '{}': missing `type`", view, name))
    })?;
    let size = match parsed.size {
        None | Some(YamlValue::Null) => None,
        Some(YamlValue::String(s)) => Some(s),
        Some(YamlValue::Number(n)) => Some(n.to_string()),
        Some(other) => {
            return Err(ViewTypesError::Schema(format!(
                "view '{}' field '{}': size must be a string or number (got {:?})",
                view, name, other
            )));
        }
    };
    Ok(ViewFieldDef {
        name,
        r#type: type_,
        size,
        references: parsed.references,
        is_nullable: parsed.is_nullable.unwrap_or(false),
    })
}

fn take_single_keyed_map(
    raw: YamlValue,
    context: &str,
) -> Result<(String, YamlValue), ViewTypesError> {
    match raw {
        YamlValue::Mapping(map) => {
            if map.len() != 1 {
                return Err(ViewTypesError::Schema(format!(
                    "{}: expected single-key map (got {} keys)",
                    context,
                    map.len()
                )));
            }
            let (k, v) = map.into_iter().next().unwrap();
            let name = match k {
                YamlValue::String(s) => s,
                _ => {
                    return Err(ViewTypesError::Schema(format!(
                        "{}: key must be a string",
                        context
                    )));
                }
            };
            Ok((name, v))
        }
        other => Err(ViewTypesError::Schema(format!(
            "{}: expected mapping (got {:?})",
            context, other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_yaml_returns_empty_doc() {
        let doc = parse_view_types("").unwrap();
        assert!(doc.types.is_empty());
        assert!(!doc.datasource_inclusion.auto_enrich);
    }

    #[test]
    fn parses_inclusion_block() {
        let yaml = concat!(
            "datasource_types:\n",
            "  include: \"*\"\n",
            "  filter: \"type.datasource_type != \\\"many-to-many\\\"\"\n",
            "  auto_enrich: true\n",
            "types: []\n",
        );
        let doc = parse_view_types(yaml).unwrap();
        assert_eq!(doc.datasource_inclusion.include.as_deref(), Some("*"));
        assert!(doc.datasource_inclusion.auto_enrich);
        assert!(doc.datasource_inclusion.filter.is_some());
    }

    #[test]
    fn parses_inclusion_authored_under_includes_directive() {
        let yaml = concat!(
            "includes:\n",
            "  - datasource_types:\n",
            "      include: \"*\"\n",
            "      filter: \"type.datasource_type != \\\"many-to-many\\\"\"\n",
            "      auto_enrich: true\n",
            "types: []\n",
        );
        let doc = parse_view_types(yaml).unwrap();
        assert_eq!(doc.datasource_inclusion.include.as_deref(), Some("*"));
        assert!(
            doc.datasource_inclusion.auto_enrich,
            "auto_enrich must be read from the `includes:` directive the emitters use, else enriched responses keep the raw FK id the OpenAPI schema forbids"
        );
        assert!(doc.datasource_inclusion.filter.is_some());
    }

    #[test]
    fn includes_scan_skips_non_datasource_directives() {
        let yaml = concat!(
            "includes:\n",
            "  - view_type_routes:\n",
            "      include: \"*\"\n",
            "  - datasource_types:\n",
            "      auto_enrich: true\n",
            "types: []\n",
        );
        let doc = parse_view_types(yaml).unwrap();
        assert!(doc.datasource_inclusion.auto_enrich);
    }

    #[test]
    fn parses_inherits_view() {
        let yaml = concat!(
            "types:\n",
            "  - contact:\n",
            "      inherits: datasource_types.contact\n",
            "      fields:\n",
            "        - addresses:\n",
            "            type: datasource_types.address[]\n",
            "            references: datasource_types.address.contact_id\n",
        );
        let doc = parse_view_types(yaml).unwrap();
        let v = &doc.types[0];
        assert_eq!(v.name, "contact");
        assert_eq!(v.inherits.as_deref(), Some("datasource_types.contact"));
        assert_eq!(v.fields.len(), 1);
        assert_eq!(v.fields[0].name, "addresses");
        assert_eq!(v.fields[0].r#type, "datasource_types.address[]");
        assert_eq!(
            v.fields[0].references.as_deref(),
            Some("datasource_types.address.contact_id"),
        );
    }

    #[test]
    fn parses_standalone_request_response_views() {
        let yaml = concat!(
            "types:\n",
            "  - import_contacts_request:\n",
            "      fields:\n",
            "        - source:\n",
            "            type: string\n",
            "            size: 64\n",
            "        - payload:\n",
            "            type: string\n",
            "            size: unlimited\n",
        );
        let doc = parse_view_types(yaml).unwrap();
        let v = &doc.types[0];
        assert!(v.inherits.is_none());
        assert_eq!(v.fields[0].size.as_deref(), Some("64"));
        assert_eq!(v.fields[1].size.as_deref(), Some("unlimited"));
    }

    #[test]
    fn missing_field_type_errors() {
        let yaml = concat!(
            "types:\n",
            "  - bad:\n",
            "      fields:\n",
            "        - x:\n",
            "            size: 64\n",
        );
        let err = parse_view_types(yaml).unwrap_err();
        assert!(format!("{}", err).contains("missing `type`"));
    }
}
