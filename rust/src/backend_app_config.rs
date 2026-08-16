use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiddlewareEntry {
    pub name: String,
    pub r#type: String,
    pub enabled: bool,
    pub apply_routes: Option<Vec<String>>,
    pub deny_routes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerEntry {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticEntry {
    pub path: String,
    pub dir: String,
}

pub const DEFAULT_HANDLERS: &[&str] =
    &["NotFoundMiddlewareService", "ErrorHandlerMiddlewareService"];

pub const DEFAULT_MIDDLEWARE: &[&str] = &["securityHeaders", "cors", "jsonBody", "formBody"];

// Mirrors buildAppModel's `backendAppConfig?.name || "generated-app"` (create-backend-app-model.mjs) so both lanes log the same name when backend-app.yaml omits it.
pub const DEFAULT_APP_NAME: &str = "generated-app";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendAppConfig {
    pub name: String,
    pub middleware: Vec<MiddlewareEntry>,
    pub handlers: Vec<HandlerEntry>,
    pub statics: Option<Vec<StaticEntry>>,
}

impl Default for BackendAppConfig {
    fn default() -> Self {
        Self {
            name: DEFAULT_APP_NAME.to_string(),
            middleware: Vec::new(),
            handlers: Vec::new(),
            statics: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum BackendAppConfigError {
    #[error("io error reading app config: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("backend-app.yaml: {0}")]
    Schema(String),
}

#[derive(Clone, Copy)]
struct MiddlewareRegistryEntry {
    name: &'static str,
    r#type: &'static str,
}

const MIDDLEWARE_REGISTRY: &[MiddlewareRegistryEntry] = &[
    MiddlewareRegistryEntry {
        name: "cors",
        r#type: "app",
    },
    MiddlewareRegistryEntry {
        name: "securityHeaders",
        r#type: "app",
    },
    MiddlewareRegistryEntry {
        name: "jsonBody",
        r#type: "app",
    },
    MiddlewareRegistryEntry {
        name: "largeJsonBody",
        r#type: "app",
    },
    MiddlewareRegistryEntry {
        name: "formBody",
        r#type: "app",
    },
    MiddlewareRegistryEntry {
        name: "errorHandler",
        r#type: "app",
    },
    MiddlewareRegistryEntry {
        name: "authenticate",
        r#type: "route",
    },
    MiddlewareRegistryEntry {
        name: "authenticateSignin",
        r#type: "route",
    },
    MiddlewareRegistryEntry {
        name: "authorize",
        r#type: "route",
    },
    MiddlewareRegistryEntry {
        name: "validateBody",
        r#type: "route",
    },
    MiddlewareRegistryEntry {
        name: "validateParams",
        r#type: "route",
    },
    MiddlewareRegistryEntry {
        name: "protectBuiltinRow",
        r#type: "route",
    },
    MiddlewareRegistryEntry {
        name: "traceRoute",
        r#type: "route",
    },
    MiddlewareRegistryEntry {
        name: "traceService",
        r#type: "service",
    },
    MiddlewareRegistryEntry {
        name: "traceDatasource",
        r#type: "datasource",
    },
];

fn registry_lookup(name: &str) -> Option<&'static MiddlewareRegistryEntry> {
    MIDDLEWARE_REGISTRY.iter().find(|e| e.name == name)
}

#[derive(Deserialize)]
struct MiddlewareOptions {
    #[serde(rename = "type")]
    r#type: Option<String>,
    enabled: Option<bool>,
    apply_routes: Option<Vec<String>>,
    deny_routes: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct HandlerOptions {
    enabled: Option<bool>,
}

fn normalize_item(raw: &YamlValue) -> Result<MiddlewareEntry, BackendAppConfigError> {
    match raw {
        YamlValue::String(name) => {
            let registry_type = registry_lookup(name)
                .map(|r| r.r#type.to_string())
                .unwrap_or_else(|| "app".to_string());
            Ok(MiddlewareEntry {
                name: name.clone(),
                r#type: registry_type,
                enabled: true,
                apply_routes: None,
                deny_routes: None,
            })
        }
        YamlValue::Mapping(map) => {
            if map.len() != 1 {
                return Err(BackendAppConfigError::Schema(
                    "middleware item map must have exactly one key".to_string(),
                ));
            }
            let (key, value) = map.iter().next().unwrap();
            let name = match key {
                YamlValue::String(s) => s.clone(),
                _ => {
                    return Err(BackendAppConfigError::Schema(
                        "middleware item key must be a string".to_string(),
                    ));
                }
            };
            let opts: MiddlewareOptions = serde_yaml::from_value(value.clone())?;
            let registry = registry_lookup(&name);
            if registry.is_none() {
                return Err(BackendAppConfigError::Schema(format!(
                    "unknown middleware name \"{}\"",
                    name
                )));
            }
            let r#type = opts
                .r#type
                .unwrap_or_else(|| registry.unwrap().r#type.to_string());
            let enabled = opts.enabled.unwrap_or(true);
            let apply_routes = validate_route_list(&name, "apply_routes", opts.apply_routes)?;
            let deny_routes = validate_route_list(&name, "deny_routes", opts.deny_routes)?;
            if apply_routes.is_some() && deny_routes.is_some() {
                return Err(BackendAppConfigError::Schema(format!(
                    "apply_routes and deny_routes are mutually exclusive on a single middleware item (\"{}\")",
                    name
                )));
            }
            Ok(MiddlewareEntry {
                name,
                r#type,
                enabled,
                apply_routes,
                deny_routes,
            })
        }
        _ => Err(BackendAppConfigError::Schema(
            "middleware item must be a string or single-key map".to_string(),
        )),
    }
}

fn validate_route_list(
    middleware_name: &str,
    field: &str,
    raw: Option<Vec<String>>,
) -> Result<Option<Vec<String>>, BackendAppConfigError> {
    let Some(list) = raw else {
        return Ok(None);
    };
    if list.is_empty() {
        return Err(BackendAppConfigError::Schema(format!(
            "middleware \"{}\": {} must have at least one entry when present",
            middleware_name, field
        )));
    }
    for entry in &list {
        if entry.is_empty() {
            return Err(BackendAppConfigError::Schema(format!(
                "middleware \"{}\": {} entry must be non-empty",
                middleware_name, field
            )));
        }
        if !entry.starts_with('/') {
            return Err(BackendAppConfigError::Schema(format!(
                "middleware \"{}\": {} entry \"{}\" must start with \"/\"",
                middleware_name, field, entry
            )));
        }
    }
    Ok(Some(list))
}

fn normalize_handler_item(raw: &YamlValue) -> Result<HandlerEntry, BackendAppConfigError> {
    match raw {
        YamlValue::String(name) => Ok(HandlerEntry {
            name: name.clone(),
            enabled: true,
        }),
        YamlValue::Mapping(map) => {
            if map.len() != 1 {
                return Err(BackendAppConfigError::Schema(
                    "handler item map must have exactly one key (the handler name)".to_string(),
                ));
            }
            let (key, value) = map.iter().next().unwrap();
            let name = match key {
                YamlValue::String(s) => s.clone(),
                _ => {
                    return Err(BackendAppConfigError::Schema(
                        "handler item key must be a string".to_string(),
                    ));
                }
            };
            let opts: HandlerOptions = serde_yaml::from_value(value.clone())?;
            let enabled = opts.enabled.unwrap_or(true);
            Ok(HandlerEntry { name, enabled })
        }
        _ => Err(BackendAppConfigError::Schema(
            "handler item must be a string or single-key map".to_string(),
        )),
    }
}

fn merge_middleware_defaults(declared: Vec<MiddlewareEntry>) -> Vec<MiddlewareEntry> {
    let declared_names: HashSet<String> = declared.iter().map(|m| m.name.clone()).collect();
    let mut merged: Vec<MiddlewareEntry> = DEFAULT_MIDDLEWARE
        .iter()
        .filter(|name| !declared_names.contains(**name))
        .map(|name| MiddlewareEntry {
            name: name.to_string(),
            r#type: "app".to_string(),
            enabled: true,
            apply_routes: None,
            deny_routes: None,
        })
        .collect();
    merged.extend(declared);
    merged
}

fn merge_handler_defaults(declared: Vec<HandlerEntry>) -> Vec<HandlerEntry> {
    let declared_names: HashSet<String> = declared.iter().map(|h| h.name.clone()).collect();
    let mut merged: Vec<HandlerEntry> = DEFAULT_HANDLERS
        .iter()
        .filter(|name| !declared_names.contains(**name))
        .map(|name| HandlerEntry {
            name: name.to_string(),
            enabled: true,
        })
        .collect();
    merged.extend(declared);
    merged
}

fn parse_doc(doc: YamlValue) -> Result<BackendAppConfig, BackendAppConfigError> {
    let map = match doc {
        YamlValue::Mapping(m) => m,
        YamlValue::Null => {
            return Ok(BackendAppConfig {
                name: DEFAULT_APP_NAME.to_string(),
                middleware: merge_middleware_defaults(Vec::new()),
                handlers: merge_handler_defaults(Vec::new()),
                statics: None,
            });
        }
        _ => {
            return Err(BackendAppConfigError::Schema(
                "top-level backend-app.yaml must be a mapping".to_string(),
            ));
        }
    };
    let name = match map.get(YamlValue::String("name".to_string())) {
        None | Some(YamlValue::Null) => DEFAULT_APP_NAME.to_string(),
        Some(YamlValue::String(s)) if !s.is_empty() => s.clone(),
        Some(YamlValue::String(_)) => {
            return Err(BackendAppConfigError::Schema(
                "name must be a non-empty string".to_string(),
            ));
        }
        _ => {
            return Err(BackendAppConfigError::Schema(
                "name must be a string".to_string(),
            ));
        }
    };
    let middleware_raw = map.get(YamlValue::String("middleware".to_string()));
    let declared_middleware = match middleware_raw {
        None | Some(YamlValue::Null) => Vec::new(),
        Some(YamlValue::Sequence(items)) => {
            let mut seen: HashSet<String> = HashSet::new();
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let entry = normalize_item(item)?;
                if !seen.insert(entry.name.clone()) {
                    return Err(BackendAppConfigError::Schema(format!(
                        "duplicate middleware name \"{}\"",
                        entry.name
                    )));
                }
                out.push(entry);
            }
            out
        }
        _ => {
            return Err(BackendAppConfigError::Schema(
                "middleware must be a sequence".to_string(),
            ));
        }
    };
    let middleware = merge_middleware_defaults(declared_middleware);
    let handlers_raw = map.get(YamlValue::String("handlers".to_string()));
    let declared_handlers = match handlers_raw {
        None | Some(YamlValue::Null) => Vec::new(),
        Some(YamlValue::Sequence(items)) => {
            let mut seen: HashSet<String> = HashSet::new();
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let entry = normalize_handler_item(item)?;
                if !seen.insert(entry.name.clone()) {
                    return Err(BackendAppConfigError::Schema(format!(
                        "duplicate handler name \"{}\"",
                        entry.name
                    )));
                }
                out.push(entry);
            }
            out
        }
        _ => {
            return Err(BackendAppConfigError::Schema(
                "handlers must be a sequence".to_string(),
            ));
        }
    };
    let statics_raw = map.get(YamlValue::String("statics".to_string()));
    let statics = match statics_raw {
        None | Some(YamlValue::Null) => None,
        Some(YamlValue::Sequence(items)) => {
            if items.is_empty() {
                return Err(BackendAppConfigError::Schema(
                    "statics must have at least one entry when present".to_string(),
                ));
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(normalize_static_item(item)?);
            }
            Some(out)
        }
        _ => {
            return Err(BackendAppConfigError::Schema(
                "statics must be a sequence".to_string(),
            ));
        }
    };
    Ok(BackendAppConfig {
        name,
        middleware,
        handlers: merge_handler_defaults(declared_handlers),
        statics,
    })
}

#[derive(Deserialize)]
struct StaticOptions {
    path: String,
    dir: String,
}

fn normalize_static_item(raw: &YamlValue) -> Result<StaticEntry, BackendAppConfigError> {
    let opts: StaticOptions = serde_yaml::from_value(raw.clone())
        .map_err(|e| BackendAppConfigError::Schema(format!("statics item: {}", e)))?;
    if opts.path.is_empty() {
        return Err(BackendAppConfigError::Schema(
            "statics entry path must be non-empty".to_string(),
        ));
    }
    if opts.dir.is_empty() {
        return Err(BackendAppConfigError::Schema(
            "statics entry dir must be non-empty".to_string(),
        ));
    }
    Ok(StaticEntry {
        path: opts.path,
        dir: opts.dir,
    })
}

pub fn load_backend_app_config(path: &Path) -> Result<BackendAppConfig, BackendAppConfigError> {
    let text = std::fs::read_to_string(path)?;
    let doc: YamlValue = serde_yaml::from_str(&text)?;
    parse_doc(doc)
}

pub fn parse_enable_middleware_env(value: Option<&str>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for raw in value.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let mapped = match token.to_ascii_lowercase().as_str() {
            "route" => "traceRoute".to_string(),
            "service" => "traceService".to_string(),
            "datasource" => "traceDatasource".to_string(),
            _ => token.to_string(),
        };
        out.push(mapped);
    }
    out
}

pub fn apply_enable_middleware(entries: &mut Vec<MiddlewareEntry>, enable: &[String]) {
    if enable.is_empty() {
        return;
    }
    let present: HashSet<String> = entries.iter().map(|e| e.name.clone()).collect();
    let enable_set: HashSet<&str> = enable.iter().map(|s| s.as_str()).collect();
    for entry in entries.iter_mut() {
        if enable_set.contains(entry.name.as_str()) {
            entry.enabled = true;
        }
    }
    for name in enable {
        if present.contains(name) {
            continue;
        }
        let Some(registry) = registry_lookup(name) else {
            continue;
        };
        entries.push(MiddlewareEntry {
            name: name.clone(),
            r#type: registry.r#type.to_string(),
            enabled: true,
            apply_routes: None,
            deny_routes: None,
        });
    }
}

pub fn is_middleware_enabled(config: &BackendAppConfig, name: &str) -> bool {
    config
        .middleware
        .iter()
        .any(|e| e.name == name && e.enabled)
}

pub fn is_handler_enabled(config: &BackendAppConfig, name: &str) -> bool {
    config.handlers.iter().any(|e| e.name == name && e.enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile_shim::NamedTempFile;

    mod tempfile_shim {
        use std::fs;
        use std::io::{self, Write};
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        pub struct NamedTempFile {
            path: PathBuf,
        }

        impl NamedTempFile {
            pub fn new() -> io::Result<Self> {
                let pid = std::process::id();
                let n = COUNTER.fetch_add(1, Ordering::SeqCst);
                let path = std::env::temp_dir().join(format!("app-cfg-{}-{}.yaml", pid, n));
                fs::File::create(&path)?;
                Ok(NamedTempFile { path })
            }
            pub fn path(&self) -> &Path {
                &self.path
            }
            pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(&self.path)?;
                f.write_all(bytes)
            }
        }

        impl Drop for NamedTempFile {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    fn parse_str(yaml: &str) -> Result<BackendAppConfig, BackendAppConfigError> {
        let doc: YamlValue = serde_yaml::from_str(yaml)?;
        parse_doc(doc)
    }

    fn find_by_name<'a>(list: &'a [MiddlewareEntry], name: &str) -> &'a MiddlewareEntry {
        list.iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("expected middleware entry \"{}\"", name))
    }

    #[test]
    fn parse_bare_string_treats_entry_as_enabled() {
        let cfg = parse_str("middleware:\n  - traceRoute\n").unwrap();
        let entry = find_by_name(&cfg.middleware, "traceRoute");
        assert_eq!(entry.r#type, "route");
        assert!(entry.enabled);
    }

    #[test]
    fn parse_map_form_respects_enabled_field() {
        let on =
            parse_str("middleware:\n  - traceRoute:\n      type: route\n      enabled: true\n")
                .unwrap();
        assert!(find_by_name(&on.middleware, "traceRoute").enabled);

        let off =
            parse_str("middleware:\n  - traceRoute:\n      type: route\n      enabled: false\n")
                .unwrap();
        assert!(!find_by_name(&off.middleware, "traceRoute").enabled);
    }

    #[test]
    fn parse_full_three_entry_block_matches_scaffold() {
        let scaffold = [
            "middleware:",
            "  - traceRoute:",
            "      type: route",
            "      enabled: false",
            "  - traceService:",
            "      type: service",
            "      enabled: false",
            "  - traceDatasource:",
            "      type: datasource",
            "      enabled: false",
            "",
        ]
        .join("\n");
        let cfg = parse_str(&scaffold).unwrap();
        let tr = find_by_name(&cfg.middleware, "traceRoute");
        assert_eq!(tr.r#type, "route");
        assert!(!tr.enabled);
        let ts = find_by_name(&cfg.middleware, "traceService");
        assert_eq!(ts.r#type, "service");
        assert!(!ts.enabled);
        let td = find_by_name(&cfg.middleware, "traceDatasource");
        assert_eq!(td.r#type, "datasource");
        assert!(!td.enabled);
    }

    #[test]
    fn parse_missing_middleware_key_merges_defaults() {
        let cfg = parse_str("handlers:\n  - CustomHandler\n").unwrap();
        assert_eq!(cfg.middleware.len(), DEFAULT_MIDDLEWARE.len());
        for name in DEFAULT_MIDDLEWARE {
            let entry = find_by_name(&cfg.middleware, name);
            assert!(entry.enabled);
            assert_eq!(entry.r#type, "app");
        }
    }

    #[test]
    fn parse_null_yaml_merges_default_middleware() {
        let cfg = parse_str("").unwrap();
        assert_eq!(cfg.middleware.len(), DEFAULT_MIDDLEWARE.len());
        for name in DEFAULT_MIDDLEWARE {
            assert!(
                is_middleware_enabled(&cfg, name),
                "missing default {}",
                name
            );
        }
    }

    #[test]
    fn defaults_precede_user_declared_entries_in_merged_middleware_list() {
        let cfg = parse_str("middleware:\n  - traceRoute\n").unwrap();
        let names: Vec<&str> = cfg.middleware.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "securityHeaders",
                "cors",
                "jsonBody",
                "formBody",
                "traceRoute"
            ]
        );
    }

    #[test]
    fn declared_middleware_shadows_default_at_same_name() {
        let cfg = parse_str("middleware:\n  - cors:\n      enabled: false\n").unwrap();
        let cors = find_by_name(&cfg.middleware, "cors");
        assert!(
            !cors.enabled,
            "declared cors:enabled=false must win over default"
        );
        let names: Vec<&str> = cfg.middleware.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names.iter().filter(|n| **n == "cors").count(), 1);
    }

    #[test]
    fn parse_enable_middleware_env_translates_tier_shortcuts() {
        assert_eq!(
            parse_enable_middleware_env(Some("route")),
            vec!["traceRoute".to_string()]
        );
        assert_eq!(
            parse_enable_middleware_env(Some("route,service")),
            vec!["traceRoute".to_string(), "traceService".to_string()]
        );
        assert_eq!(
            parse_enable_middleware_env(Some("DETERMINISTIC_TRACE_CUSTOM")),
            vec!["DETERMINISTIC_TRACE_CUSTOM".to_string()]
        );
        assert!(parse_enable_middleware_env(None).is_empty());
        assert!(parse_enable_middleware_env(Some("")).is_empty());
    }

    #[test]
    fn apply_enable_middleware_flips_present_entries() {
        let mut entries = vec![MiddlewareEntry {
            name: "traceRoute".to_string(),
            r#type: "route".to_string(),
            enabled: false,
            apply_routes: None,
            deny_routes: None,
        }];
        apply_enable_middleware(&mut entries, &["traceRoute".to_string()]);
        assert!(entries[0].enabled);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn apply_enable_middleware_appends_missing_entries() {
        let mut entries: Vec<MiddlewareEntry> = Vec::new();
        apply_enable_middleware(&mut entries, &["traceRoute".to_string()]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "traceRoute");
        assert_eq!(entries[0].r#type, "route");
        assert!(entries[0].enabled);
    }

    #[test]
    fn apply_enable_middleware_skips_unknown_names_when_appending() {
        let mut entries: Vec<MiddlewareEntry> = Vec::new();
        apply_enable_middleware(&mut entries, &["customUnknown".to_string()]);
        assert!(entries.is_empty());
    }

    #[test]
    fn is_middleware_enabled_returns_false_for_unknown_names() {
        let cfg = BackendAppConfig::default();
        assert!(!is_middleware_enabled(&cfg, "traceRoute"));
    }

    #[test]
    fn is_middleware_enabled_returns_false_when_present_but_disabled() {
        let cfg = BackendAppConfig {
            name: DEFAULT_APP_NAME.to_string(),
            middleware: vec![MiddlewareEntry {
                name: "traceRoute".to_string(),
                r#type: "route".to_string(),
                enabled: false,
                apply_routes: None,
                deny_routes: None,
            }],
            handlers: Vec::new(),
            statics: None,
        };
        assert!(!is_middleware_enabled(&cfg, "traceRoute"));
    }

    #[test]
    fn is_middleware_enabled_returns_true_when_enabled() {
        let cfg = BackendAppConfig {
            name: DEFAULT_APP_NAME.to_string(),
            middleware: vec![MiddlewareEntry {
                name: "traceRoute".to_string(),
                r#type: "route".to_string(),
                enabled: true,
                apply_routes: None,
                deny_routes: None,
            }],
            handlers: Vec::new(),
            statics: None,
        };
        assert!(is_middleware_enabled(&cfg, "traceRoute"));
    }

    #[test]
    fn handlers_default_when_missing_from_yaml() {
        let cfg = parse_str("middleware:\n  - traceRoute\n").unwrap();
        assert_eq!(cfg.handlers.len(), DEFAULT_HANDLERS.len());
        assert!(is_handler_enabled(&cfg, "NotFoundMiddlewareService"));
        assert!(is_handler_enabled(&cfg, "ErrorHandlerMiddlewareService"));
    }

    #[test]
    fn handlers_bare_string_treats_entry_as_enabled() {
        let cfg = parse_str("handlers:\n  - CustomHandler\n").unwrap();
        assert!(is_handler_enabled(&cfg, "CustomHandler"));
    }

    #[test]
    fn handlers_map_form_respects_enabled_field_false_shadows_default() {
        let cfg =
            parse_str("handlers:\n  - NotFoundMiddlewareService:\n      enabled: false\n").unwrap();
        assert!(!is_handler_enabled(&cfg, "NotFoundMiddlewareService"));
        assert!(is_handler_enabled(&cfg, "ErrorHandlerMiddlewareService"));
    }

    #[test]
    fn handlers_defaults_precede_user_declared_entries_in_merged_list() {
        let cfg = parse_str("handlers:\n  - CustomHandler\n").unwrap();
        let names: Vec<&str> = cfg.handlers.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "NotFoundMiddlewareService",
                "ErrorHandlerMiddlewareService",
                "CustomHandler"
            ]
        );
    }

    #[test]
    fn handlers_duplicate_name_is_a_schema_error() {
        let err = parse_str(
            "handlers:\n  - NotFoundMiddlewareService\n  - NotFoundMiddlewareService:\n      enabled: false\n",
        )
        .unwrap_err();
        assert!(
            format!("{}", err).contains("duplicate handler name \"NotFoundMiddlewareService\""),
            "{}",
            err
        );
    }

    #[test]
    fn null_yaml_still_merges_default_handlers() {
        let cfg = parse_str("").unwrap();
        assert!(is_handler_enabled(&cfg, "NotFoundMiddlewareService"));
        assert!(is_handler_enabled(&cfg, "ErrorHandlerMiddlewareService"));
    }

    #[test]
    fn statics_default_none_when_missing_from_yaml() {
        let cfg = parse_str("middleware:\n  - traceRoute\n").unwrap();
        assert_eq!(cfg.statics, None);
    }

    #[test]
    fn statics_parses_full_entry() {
        let cfg = parse_str("statics:\n  - path: /assets\n    dir: ./public\n").unwrap();
        let entries = cfg.statics.expect("statics parsed");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/assets");
        assert_eq!(entries[0].dir, "./public");
    }

    #[test]
    fn statics_parses_multiple_entries_preserving_order() {
        let cfg =
            parse_str("statics:\n  - path: /a\n    dir: ./a-dir\n  - path: /b\n    dir: ./b-dir\n")
                .unwrap();
        let entries = cfg.statics.expect("statics parsed");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "/a");
        assert_eq!(entries[1].path, "/b");
    }

    #[test]
    fn statics_rejects_empty_list() {
        let err = parse_str("statics: []\n").unwrap_err();
        assert!(
            format!("{}", err).contains("statics must have at least one entry"),
            "{}",
            err
        );
    }

    #[test]
    fn statics_rejects_missing_path() {
        let err = parse_str("statics:\n  - dir: ./public\n").unwrap_err();
        assert!(format!("{}", err).contains("statics item"), "{}", err);
    }

    #[test]
    fn statics_rejects_missing_dir() {
        let err = parse_str("statics:\n  - path: /assets\n").unwrap_err();
        assert!(format!("{}", err).contains("statics item"), "{}", err);
    }

    #[test]
    fn statics_rejects_empty_path() {
        let err = parse_str("statics:\n  - path: \"\"\n    dir: ./public\n").unwrap_err();
        assert!(
            format!("{}", err).contains("statics entry path must be non-empty"),
            "{}",
            err
        );
    }

    #[test]
    fn statics_rejects_empty_dir() {
        let err = parse_str("statics:\n  - path: /assets\n    dir: \"\"\n").unwrap_err();
        assert!(
            format!("{}", err).contains("statics entry dir must be non-empty"),
            "{}",
            err
        );
    }

    #[test]
    fn middleware_apply_routes_and_deny_routes_default_to_none() {
        let cfg = parse_str("middleware:\n  - traceRoute\n").unwrap();
        let entry = find_by_name(&cfg.middleware, "traceRoute");
        assert_eq!(entry.apply_routes, None);
        assert_eq!(entry.deny_routes, None);
    }

    #[test]
    fn middleware_apply_routes_parses_and_populates_field() {
        let cfg = parse_str(
            "middleware:\n  - authenticate:\n      apply_routes:\n        - /api/protected\n        - /api/admin\n",
        )
        .unwrap();
        let entry = find_by_name(&cfg.middleware, "authenticate");
        assert_eq!(
            entry.apply_routes,
            Some(vec!["/api/protected".to_string(), "/api/admin".to_string()])
        );
        assert_eq!(entry.deny_routes, None);
    }

    #[test]
    fn middleware_deny_routes_parses_and_populates_field() {
        let cfg = parse_str(
            "middleware:\n  - authenticate:\n      deny_routes:\n        - /api/public\n",
        )
        .unwrap();
        let entry = find_by_name(&cfg.middleware, "authenticate");
        assert_eq!(entry.deny_routes, Some(vec!["/api/public".to_string()]));
        assert_eq!(entry.apply_routes, None);
    }

    #[test]
    fn middleware_apply_and_deny_together_is_rejected() {
        let err = parse_str(
            "middleware:\n  - authenticate:\n      apply_routes:\n        - /api\n      deny_routes:\n        - /health\n",
        )
        .unwrap_err();
        assert!(format!("{}", err).contains("mutually exclusive"), "{}", err);
    }

    #[test]
    fn middleware_apply_routes_rejects_empty_list() {
        let err =
            parse_str("middleware:\n  - authenticate:\n      apply_routes: []\n").unwrap_err();
        assert!(
            format!("{}", err).contains("apply_routes must have at least one entry"),
            "{}",
            err
        );
    }

    #[test]
    fn middleware_apply_routes_rejects_non_slash_prefix() {
        let err = parse_str(
            "middleware:\n  - authenticate:\n      apply_routes:\n        - api/no-slash\n",
        )
        .unwrap_err();
        assert!(
            format!("{}", err).contains("must start with \"/\""),
            "{}",
            err
        );
    }

    #[test]
    fn middleware_deny_routes_rejects_empty_entry() {
        let err = parse_str("middleware:\n  - authenticate:\n      deny_routes:\n        - \"\"\n")
            .unwrap_err();
        assert!(
            format!("{}", err).contains("deny_routes entry must be non-empty"),
            "{}",
            err
        );
    }

    #[test]
    fn load_backend_app_config_throws_on_unknown_middleware_name() {
        let f = NamedTempFile::new().unwrap();
        f.write_all(
            b"middleware:\n  - thisDoesNotExist:\n      type: route\n      enabled: true\n",
        )
        .unwrap();
        let err = load_backend_app_config(f.path()).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown middleware name"), "{}", msg);
    }

    #[test]
    fn load_backend_app_config_reads_real_file() {
        let f = NamedTempFile::new().unwrap();
        f.write_all(b"middleware:\n  - traceRoute:\n      type: route\n      enabled: true\n")
            .unwrap();
        let cfg = load_backend_app_config(f.path()).unwrap();
        assert!(is_middleware_enabled(&cfg, "traceRoute"));
    }

    #[test]
    fn reads_declared_app_name() {
        let f = NamedTempFile::new().unwrap();
        f.write_all(b"name: acme-api\n").unwrap();
        let cfg = load_backend_app_config(f.path()).unwrap();
        assert_eq!(cfg.name, "acme-api");
    }

    #[test]
    fn defaults_app_name_when_omitted() {
        let f = NamedTempFile::new().unwrap();
        f.write_all(b"middleware:\n  - traceRoute\n").unwrap();
        let cfg = load_backend_app_config(f.path()).unwrap();
        assert_eq!(cfg.name, DEFAULT_APP_NAME);
    }

    #[test]
    fn rejects_empty_app_name() {
        let f = NamedTempFile::new().unwrap();
        f.write_all(b"name: \"\"\n").unwrap();
        let err = load_backend_app_config(f.path()).unwrap_err();
        assert!(format!("{}", err).contains("name must be a non-empty string"));
    }
}
