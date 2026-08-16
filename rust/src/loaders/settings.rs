use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::id_type::IdType;

/// How `datetime` values are represented in storage. Native lets the driver own the type;
/// String stores an ISO-8601 text column. Mirrors the JS `DatasourceSettings.datetimeRepr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatetimeRepr {
    #[default]
    Native,
    String,
}

/// How `uuid` values are represented in storage. Mirrors the JS `DatasourceSettings.uuidRepr`
/// (`string` default vs a driver-native uuid type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UuidRepr {
    #[default]
    String,
    Native,
}

/// The resolved `settings.datasource` block — the single source of truth for every
/// datasource-setting-derived runtime decision (id generation, table pluralization,
/// optimistic concurrency, stored procedures, datetime/uuid representation). Mirrors the
/// emitter-side `DatasourceSettings` class so the runtime and the generated code agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasourceSettings {
    pub id_type: IdType,
    pub uuid_repr: UuidRepr,
    pub datetime_repr: DatetimeRepr,
    pub pluralize_table_names: bool,
    pub use_stored_procedures: bool,
    pub use_optimistic_concurrency: bool,
}

impl Default for DatasourceSettings {
    fn default() -> Self {
        Self {
            id_type: IdType::Integer,
            uuid_repr: UuidRepr::String,
            datetime_repr: DatetimeRepr::Native,
            pluralize_table_names: true,
            use_stored_procedures: false,
            use_optimistic_concurrency: false,
        }
    }
}

impl DatasourceSettings {
    pub fn is_uuid(&self) -> bool {
        self.id_type.is_uuid()
    }

    /// A uuid primary key IS the uuid, so no separate system `uuid` column is carried.
    pub fn with_uuid_column(&self) -> bool {
        !self.is_uuid()
    }

    pub fn is_native_datetime(&self) -> bool {
        self.datetime_repr == DatetimeRepr::Native
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
    id_type: Option<String>,
    uuid: Option<String>,
    datetime: Option<String>,
    use_stored_procedures: Option<bool>,
    use_optimistic_concurrency: Option<bool>,
}

fn uuid_repr_from(raw: Option<&str>) -> UuidRepr {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("native") => UuidRepr::Native,
        _ => UuidRepr::String,
    }
}

fn datetime_repr_from(raw: Option<&str>) -> DatetimeRepr {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("string") => DatetimeRepr::String,
        _ => DatetimeRepr::Native,
    }
}

pub fn parse_settings_config(yaml: &str) -> Result<SettingsConfig, SettingsConfigError> {
    if yaml.trim().is_empty() {
        return Ok(SettingsConfig::default());
    }
    let doc: SettingsDoc = serde_yaml::from_str(yaml)?;
    let ds = &doc.settings.datasource;
    let datasource = DatasourceSettings {
        id_type: ds
            .id_type
            .as_deref()
            .map(IdType::from_settings_str)
            .unwrap_or_default(),
        uuid_repr: uuid_repr_from(ds.uuid.as_deref()),
        datetime_repr: datetime_repr_from(ds.datetime.as_deref()),
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
        assert_eq!(cfg.datasource.id_type, IdType::Integer);
    }

    #[test]
    fn default_when_key_missing() {
        let cfg = parse_settings_config("settings:\n  datasource: {}\n").unwrap();
        assert!(cfg.datasource.pluralize_table_names);
        assert_eq!(cfg.datasource.id_type, IdType::Integer);
    }

    #[test]
    fn parses_pluralize_false() {
        let yaml = "settings:\n  datasource:\n    pluralize_datatable_names: false\n";
        let cfg = parse_settings_config(yaml).unwrap();
        assert!(!cfg.datasource.pluralize_table_names);
    }

    #[test]
    fn parses_id_type_uuid() {
        let yaml = "settings:\n  datasource:\n    id_type: uuid\n";
        let cfg = parse_settings_config(yaml).unwrap();
        assert_eq!(cfg.datasource.id_type, IdType::Uuid);
        assert!(cfg.datasource.is_uuid());
        assert!(!cfg.datasource.with_uuid_column());
    }

    #[test]
    fn parses_id_type_string_and_biginteger() {
        let cfg = parse_settings_config("settings:\n  datasource:\n    id_type: string\n").unwrap();
        assert_eq!(cfg.datasource.id_type, IdType::String);
        let cfg =
            parse_settings_config("settings:\n  datasource:\n    id_type: biginteger\n").unwrap();
        assert_eq!(cfg.datasource.id_type, IdType::Integer);
    }

    #[test]
    fn parses_datetime_and_uuid_representation() {
        let yaml = "settings:\n  datasource:\n    datetime: string\n    uuid: native\n";
        let cfg = parse_settings_config(yaml).unwrap();
        assert_eq!(cfg.datasource.datetime_repr, DatetimeRepr::String);
        assert!(!cfg.datasource.is_native_datetime());
        assert_eq!(cfg.datasource.uuid_repr, UuidRepr::Native);
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

    #[test]
    fn load_returns_default_when_file_missing() {
        let cfg = load_settings_config(Path::new("/nonexistent/path/settings.yaml")).unwrap();
        assert!(cfg.datasource.pluralize_table_names);
    }

    #[test]
    fn application_name_is_none_when_absent() {
        let cfg = parse_settings_config("settings:\n  datasource: {}\n").unwrap();
        assert_eq!(cfg.application_name, None);
    }

    #[test]
    fn parses_application_name() {
        let cfg = parse_settings_config("settings:\n  application_name: my-app\n").unwrap();
        assert_eq!(cfg.application_name.as_deref(), Some("my-app"));
    }

    #[test]
    fn blank_application_name_is_none() {
        let cfg = parse_settings_config("settings:\n  application_name: '   '\n").unwrap();
        assert_eq!(cfg.application_name, None);
    }
}
