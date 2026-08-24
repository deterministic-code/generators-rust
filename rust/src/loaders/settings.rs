use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

/// The resolved `settings.datasource` block — table pluralization, optimistic concurrency,
/// and stored procedures. Primary-key and datetime/uuid representation come from field types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasourceSettings {
    pub pluralize_table_names: bool,
    pub use_stored_procedures: bool,
    pub use_optimistic_concurrency: bool,
}

impl Default for DatasourceSettings {
    fn default() -> Self {
        Self {
            pluralize_table_names: true,
            use_stored_procedures: false,
            use_optimistic_concurrency: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsConfig {
    pub datasource: DatasourceSettings,
    pub application_name: Option<String>,
}

#[derive(Debug, Error)]
pub enum SettingsConfigError {
    #[error("io error reading settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

#[derive(Deserialize, Default)]
struct SettingsDoc {
    #[serde(default)]
    settings: SettingsBlock,
}

#[derive(Deserialize, Default)]
struct SettingsBlock {
    #[serde(default)]
    datasource: DatasourceBlock,
    #[serde(default)]
    application_name: Option<String>,
}

#[derive(Deserialize, Default)]
struct DatasourceBlock {
    pluralize_datatable_names: Option<bool>,
    use_stored_procedures: Option<bool>,
    use_optimistic_concurrency: Option<bool>,
}

pub fn parse_settings_config(yaml: &str) -> Result<SettingsConfig, SettingsConfigError> {
    if yaml.trim().is_empty() {
        return Ok(SettingsConfig::default());
    }
    let doc: SettingsDoc = serde_yaml::from_str(yaml)?;
    let ds = &doc.settings.datasource;
    let datasource = DatasourceSettings {
        pluralize_table_names: ds.pluralize_datatable_names.unwrap_or(true),
        use_stored_procedures: ds.use_stored_procedures.unwrap_or(false),
        use_optimistic_concurrency: ds.use_optimistic_concurrency.unwrap_or(false),
    };
    Ok(SettingsConfig {
        datasource,
        application_name: doc
            .settings
            .application_name
            .filter(|name| !name.trim().is_empty()),
    })
}

pub fn load_settings_config(path: &Path) -> Result<SettingsConfig, SettingsConfigError> {
    if !path.exists() {
        return Ok(SettingsConfig::default());
    }
    let text = std::fs::read_to_string(path)?;
    parse_settings_config(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_when_yaml_empty() {
        let cfg = parse_settings_config("").unwrap();
        assert!(cfg.datasource.pluralize_table_names);
        assert!(!cfg.datasource.use_stored_procedures);
        assert!(!cfg.datasource.use_optimistic_concurrency);
    }

    #[test]
    fn default_when_key_missing() {
        let cfg = parse_settings_config("settings:\n  datasource: {}\n").unwrap();
        assert!(cfg.datasource.pluralize_table_names);
    }

    #[test]
    fn parses_pluralize_false() {
        let yaml = "settings:\n  datasource:\n    pluralize_datatable_names: false\n";
        let cfg = parse_settings_config(yaml).unwrap();
        assert!(!cfg.datasource.pluralize_table_names);
    }

    #[test]
    fn parses_stored_procedures_and_optimistic_concurrency() {
        let yaml = "settings:\n  datasource:\n    use_stored_procedures: true\n    use_optimistic_concurrency: true\n";
        let cfg = parse_settings_config(yaml).unwrap();
        assert!(cfg.datasource.use_stored_procedures);
        assert!(cfg.datasource.use_optimistic_concurrency);
    }

    #[test]
    fn defaults_for_stored_procedures_and_concurrency_are_false() {
        let cfg = parse_settings_config("settings:\n  datasource: {}\n").unwrap();
        assert!(!cfg.datasource.use_stored_procedures);
        assert!(!cfg.datasource.use_optimistic_concurrency);
    }
}
