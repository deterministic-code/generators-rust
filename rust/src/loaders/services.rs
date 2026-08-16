use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceSpec {
    pub name: String,
    pub args: Vec<ArgSpec>,
    pub r#type: Option<String>,
    pub module: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArgSpec {
    Repo { name: String },
    Service { name: String },
    Config { key: String },
    Undefined,
    Literal { value: serde_json::Value },
}

#[derive(Debug, Error)]
pub enum ServicesError {
    #[error("io error reading services: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("services.yaml: {0}")]
    Schema(String),
}

#[derive(Deserialize, Default)]
struct ServicesDoc {
    #[serde(default)]
    services: Vec<YamlValue>,
}

#[derive(Deserialize)]
struct RawServiceEntry {
    name: String,
    #[serde(default)]
    args: Option<Vec<YamlValue>>,
    #[serde(rename = "type", default)]
    type_: Option<String>,
    #[serde(default)]
    module: Option<String>,
}

pub fn parse_services(yaml: &str) -> Result<Vec<ServiceSpec>, ServicesError> {
    if yaml.trim().is_empty() {
        return Ok(Vec::new());
    }
    let doc: ServicesDoc = serde_yaml::from_str(yaml)?;
    let mut out = Vec::with_capacity(doc.services.len());
    for raw in doc.services {
        out.push(parse_entry(raw)?);
    }
    Ok(out)
}

pub fn load_services(path: &Path) -> Result<Vec<ServiceSpec>, ServicesError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    parse_services(&text)
}

fn parse_entry(raw: YamlValue) -> Result<ServiceSpec, ServicesError> {
    let entry: RawServiceEntry = serde_yaml::from_value(raw)
        .map_err(|e| ServicesError::Schema(format!("invalid service entry: {}", e)))?;
    let args = match entry.args {
        None => Vec::new(),
        Some(items) => items
            .into_iter()
            .map(|raw| parse_arg(raw).map_err(ServicesError::Schema))
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(ServiceSpec {
        name: entry.name,
        args,
        r#type: entry.type_,
        module: entry.module,
    })
}

pub fn parse_arg(raw: YamlValue) -> Result<ArgSpec, String> {
    match raw {
        YamlValue::Null => Ok(ArgSpec::Undefined),
        YamlValue::Mapping(ref map) if map.len() == 1 => {
            let (k, v) = map.iter().next().unwrap();
            let key = match k {
                YamlValue::String(s) => s.as_str(),
                _ => return Err("arg map key must be a string".to_string()),
            };
            let value_str = match v {
                YamlValue::String(s) => Some(s.clone()),
                _ => None,
            };
            match key {
                "repo" => Ok(ArgSpec::Repo {
                    name: value_str
                        .ok_or_else(|| "repo arg must have a string value".to_string())?,
                }),
                "service" => Ok(ArgSpec::Service {
                    name: value_str
                        .ok_or_else(|| "service arg must have a string value".to_string())?,
                }),
                "config" => Ok(ArgSpec::Config {
                    key: value_str
                        .ok_or_else(|| "config arg must have a string value".to_string())?,
                }),
                "literal" => {
                    let value: serde_json::Value = serde_yaml::from_value(v.clone())
                        .map_err(|e| format!("literal arg: {}", e))?;
                    Ok(ArgSpec::Literal { value })
                }
                other => Err(format!("unknown arg kind: {}", other)),
            }
        }
        other => {
            let value: serde_json::Value =
                serde_yaml::from_value(other).map_err(|e| format!("literal arg: {}", e))?;
            Ok(ArgSpec::Literal { value })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_yaml_returns_empty_vec() {
        let out = parse_services("").unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn parses_minimal_entry() {
        let yaml = "services:\n  - name: AlphaService\n";
        let out = parse_services(yaml).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "AlphaService");
        assert!(out[0].args.is_empty());
        assert!(out[0].r#type.is_none());
        assert!(out[0].module.is_none());
    }

    #[test]
    fn parses_entry_with_module_and_type() {
        let yaml = concat!(
            "services:\n",
            "  - name: ContactImportService\n",
            "    module: ./services/contact-import-service\n",
            "    type: custom\n",
        );
        let out = parse_services(yaml).unwrap();
        assert_eq!(
            out[0].module.as_deref(),
            Some("./services/contact-import-service")
        );
        assert_eq!(out[0].r#type.as_deref(), Some("custom"));
    }

    #[test]
    fn parses_arg_kinds() {
        let yaml = concat!(
            "services:\n",
            "  - name: Mixed\n",
            "    args:\n",
            "      - repo: notification\n",
            "      - service: AuthService\n",
            "      - config: jwt.secret\n",
            "      - literal: 42\n",
        );
        let out = parse_services(yaml).unwrap();
        assert_eq!(out[0].args.len(), 4);
        match &out[0].args[0] {
            ArgSpec::Repo { name } => assert_eq!(name, "notification"),
            _ => panic!("expected Repo"),
        }
        match &out[0].args[1] {
            ArgSpec::Service { name } => assert_eq!(name, "AuthService"),
            _ => panic!("expected Service"),
        }
        match &out[0].args[2] {
            ArgSpec::Config { key } => assert_eq!(key, "jwt.secret"),
            _ => panic!("expected Config"),
        }
        match &out[0].args[3] {
            ArgSpec::Literal { value } => assert_eq!(value, &serde_json::json!(42)),
            _ => panic!("expected Literal"),
        }
    }

    #[test]
    fn ignores_top_level_extras() {
        let yaml = concat!(
            "view_type_services:\n",
            "  filter: \"type inherits datasource_types\"\n",
            "services:\n",
            "  - name: OnlyOne\n",
        );
        let out = parse_services(yaml).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn services_empty_list() {
        let yaml = "services: []\n";
        let out = parse_services(yaml).unwrap();
        assert!(out.is_empty());
    }
}
