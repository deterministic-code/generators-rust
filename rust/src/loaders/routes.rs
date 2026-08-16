use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value as YamlValue;
use thiserror::Error;

use super::services::ArgSpec;

#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    fn parse(s: &str) -> Result<Self, RoutesError> {
        match s.to_ascii_uppercase().as_str() {
            "GET" => Ok(HttpMethod::Get),
            "POST" => Ok(HttpMethod::Post),
            "PUT" => Ok(HttpMethod::Put),
            "PATCH" => Ok(HttpMethod::Patch),
            "DELETE" => Ok(HttpMethod::Delete),
            other => Err(RoutesError::Schema(format!(
                "invalid HTTP method: {}",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResponseFormat {
    Item,
    Items,
    Raw,
}

impl ResponseFormat {
    fn parse(s: &str) -> Result<Self, RoutesError> {
        match s {
            "item" => Ok(ResponseFormat::Item),
            "items" => Ok(ResponseFormat::Items),
            "raw" => Ok(ResponseFormat::Raw),
            other => Err(RoutesError::Schema(format!(
                "invalid responseFormat: {}",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericRouteSpec {
    pub route_name: String,
    pub path: String,
    pub method: HttpMethod,
    pub service: String,
    pub service_method: String,
    pub response_format: Option<ResponseFormat>,
    pub status_code: Option<u16>,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomRouteSpec {
    pub route_name: String,
    pub path: String,
    pub method: HttpMethod,
    pub route_class: String,
    pub module: String,
    pub args: Vec<ArgSpec>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RouteStub {
    pub route_name: String,
    pub path: String,
    pub method: Option<HttpMethod>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RoutesDoc {
    pub generic: Vec<GenericRouteSpec>,
    pub custom: Vec<CustomRouteSpec>,
    pub stubs: Vec<RouteStub>,
    pub eager_write_paths: Vec<String>,
    pub eager_paths: Vec<String>,
    pub combined: Vec<RawCombinedEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawCombinedEntry {
    pub parent: String,
    pub route: String,
    pub children: Vec<RawCombinedChild>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawCombinedChild {
    pub name: String,
    pub via: Option<String>,
    pub target: Option<String>,
    pub route: Option<String>,
}

#[derive(Debug, Error)]
pub enum RoutesError {
    #[error("io error reading routes: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("routes.yaml: {0}")]
    Schema(String),
}

#[derive(Deserialize, Default)]
struct RawRoutesDoc {
    #[serde(default)]
    routes: Vec<YamlValue>,
    #[serde(default)]
    view_type_routes: Option<RawViewTypeRoutes>,
    // `view_type_routes` may sit under a top-level `includes:` directive (the #1939 form) instead of at the root; both carry the same literal eager_write_path list.
    #[serde(default)]
    includes: Vec<RawRoutesInclude>,
    #[serde(default)]
    combined_routes: Vec<YamlValue>,
}

#[derive(Deserialize, Default)]
struct RawRoutesInclude {
    #[serde(default)]
    view_type_routes: Option<RawViewTypeRoutes>,
}

#[derive(Deserialize, Default)]
struct RawViewTypeRoutes {
    #[serde(default)]
    eager_write_path: Vec<String>,
    #[serde(default)]
    eager_path: Vec<String>,
}

#[derive(Deserialize)]
struct RawRouteFields {
    path: Option<String>,
    method: Option<String>,
    service: Option<String>,
    #[serde(rename = "serviceMethod")]
    service_method: Option<String>,
    #[serde(rename = "responseFormat")]
    response_format: Option<String>,
    #[serde(rename = "statusCode")]
    status_code: Option<u16>,
    #[serde(default)]
    aliases: Option<Vec<String>>,
    #[serde(rename = "routeClass")]
    route_class: Option<String>,
    module: Option<String>,
    #[serde(default)]
    args: Option<Vec<YamlValue>>,
}

pub fn parse_routes(yaml: &str) -> Result<RoutesDoc, RoutesError> {
    if yaml.trim().is_empty() {
        return Ok(RoutesDoc::default());
    }
    let doc: RawRoutesDoc = serde_yaml::from_str(yaml)?;
    let mut out = RoutesDoc::default();
    if let Some(vtr) = doc.view_type_routes {
        out.eager_write_paths = vtr.eager_write_path;
        out.eager_paths = vtr.eager_path;
    }
    for inc in doc.includes {
        if let Some(vtr) = inc.view_type_routes {
            out.eager_write_paths.extend(vtr.eager_write_path);
            out.eager_paths.extend(vtr.eager_path);
        }
    }
    for raw in doc.routes {
        match raw {
            // Bare-string route entries (CRUD + byField shorthand) are owned by the generated
            // routers now; the runtime only needs to accept them without treating them as stubs.
            YamlValue::String(_) => {}
            YamlValue::Mapping(ref map) if map.len() == 1 => {
                let (k, v) = map.iter().next().unwrap();
                let name = match k {
                    YamlValue::String(s) => s.clone(),
                    _ => {
                        return Err(RoutesError::Schema(
                            "route entry key must be a string".to_string(),
                        ));
                    }
                };
                // why null/empty-map shorthand: matches TS parseCrudRouteSpecs — `{notifications_by_key: null}` AND `{notifications_by_key: {}}` are both map forms of the bare-string byField shorthand. YAML round-trips routinely shed `null` into `{}` (and vice versa) so handling only one form leaves users hitting a `route '...' missing path` error that's actually about an unrecognised shorthand.
                if is_empty_body(v) {
                    continue;
                }
                // why recognise-and-drop: the codegen Generate emits the verbose byField form `{name: {entity, byField, methods?}}` into routes.yaml; the generated router now mounts these, so the runtime only needs to recognise the shape to keep it out of the stub "missing path" trap.
                if is_verbose_byfield(v) {
                    continue;
                }
                let fields: RawRouteFields = serde_yaml::from_value(v.clone())
                    .map_err(|e| RoutesError::Schema(format!("route '{}': {}", name, e)))?;
                if fields.route_class.is_some() {
                    out.custom.push(build_custom(name, fields)?);
                } else if fields.service.is_some() {
                    out.generic.push(build_generic(name, fields)?);
                } else {
                    out.stubs.push(build_stub(name, fields)?);
                }
            }
            other => {
                return Err(RoutesError::Schema(format!(
                    "unexpected route entry shape: {:?}",
                    other
                )));
            }
        }
    }
    for raw in doc.combined_routes {
        out.combined.push(parse_combined_entry(raw)?);
    }
    Ok(out)
}

fn kebab_to_snake(s: &str) -> String {
    s.replace('-', "_")
}

fn parse_combined_entry(raw: YamlValue) -> Result<RawCombinedEntry, RoutesError> {
    let map = match raw {
        YamlValue::Mapping(m) if m.len() == 1 => m,
        other => {
            return Err(RoutesError::Schema(format!(
                "combined_routes entry must be a single-key map, got {:?}",
                other
            )))
        }
    };
    let (key, value) = map.into_iter().next().unwrap();
    let parent = match key {
        YamlValue::String(s) => kebab_to_snake(&s),
        _ => {
            return Err(RoutesError::Schema(
                "combined_routes entry key must be a string".to_string(),
            ))
        }
    };
    let body: RawCombinedBody = serde_yaml::from_value(value)
        .map_err(|e| RoutesError::Schema(format!("combined_routes '{}': {}", parent, e)))?;
    let route = body.route.ok_or_else(|| {
        RoutesError::Schema(format!("combined_routes '{}' missing `route`", parent))
    })?;
    let mut children = Vec::new();
    for raw_child in body.combined_types.unwrap_or_default() {
        children.push(parse_combined_child(&parent, raw_child)?);
    }
    Ok(RawCombinedEntry {
        parent,
        route,
        children,
    })
}

#[derive(Deserialize, Default)]
struct RawCombinedBody {
    route: Option<String>,
    #[serde(default)]
    combined_types: Option<Vec<YamlValue>>,
}

#[derive(Deserialize, Default)]
struct RawCombinedChildFields {
    via: Option<String>,
    target: Option<String>,
    route: Option<String>,
}

fn parse_combined_child(parent: &str, raw: YamlValue) -> Result<RawCombinedChild, RoutesError> {
    match raw {
        YamlValue::String(name) => Ok(RawCombinedChild {
            name: kebab_to_snake(&name),
            via: None,
            target: None,
            route: None,
        }),
        YamlValue::Mapping(map) if map.len() == 1 => {
            let (k, v) = map.into_iter().next().unwrap();
            let raw_name = match k {
                YamlValue::String(s) => s,
                _ => {
                    return Err(RoutesError::Schema(format!(
                        "combined_routes '{}': child key must be a string",
                        parent
                    )))
                }
            };
            let fields: RawCombinedChildFields = serde_yaml::from_value(v).map_err(|e| {
                RoutesError::Schema(format!(
                    "combined_routes '{}' child '{}': {}",
                    parent, raw_name, e
                ))
            })?;
            Ok(RawCombinedChild {
                name: kebab_to_snake(&raw_name),
                via: fields.via,
                target: fields.target,
                route: fields.route,
            })
        }
        other => Err(RoutesError::Schema(format!(
            "combined_routes '{}': unexpected child shape {:?}",
            parent, other
        ))),
    }
}

pub fn load_routes(path: &Path) -> Result<RoutesDoc, RoutesError> {
    if !path.exists() {
        return Ok(RoutesDoc::default());
    }
    let text = std::fs::read_to_string(path)?;
    parse_routes(&text)
}

fn build_generic(name: String, fields: RawRouteFields) -> Result<GenericRouteSpec, RoutesError> {
    let path = fields
        .path
        .ok_or_else(|| RoutesError::Schema(format!("route '{}' missing `path`", name)))?;
    let method = HttpMethod::parse(
        fields
            .method
            .as_deref()
            .ok_or_else(|| RoutesError::Schema(format!("route '{}' missing `method`", name)))?,
    )?;
    let service = fields.service.unwrap();
    let service_method = fields.service_method.unwrap_or_else(|| name.clone());
    let response_format = match fields.response_format {
        Some(s) => Some(ResponseFormat::parse(&s)?),
        None => None,
    };
    Ok(GenericRouteSpec {
        route_name: name,
        path,
        method,
        service,
        service_method,
        response_format,
        status_code: fields.status_code,
        aliases: fields.aliases.unwrap_or_default(),
    })
}

fn build_stub(name: String, fields: RawRouteFields) -> Result<RouteStub, RoutesError> {
    let path = fields.path.ok_or_else(|| {
        if looks_like_byfield_shorthand(&name) {
            RoutesError::Schema(format!(
                "route '{}' missing `path` — this looks like a byField shorthand. Write it as a bare string (`- {}`) or as `- {}: null` to opt into the auto-generated /api/<plural>/<field>/:<field> CRUD routes. The current `<name>: {{...}}` body has no `path:` field, so it's parsed as a stub instead.",
                name, name, name
            ))
        } else {
            RoutesError::Schema(format!("route '{}' missing `path`", name))
        }
    })?;
    let method = match fields.method {
        Some(s) => Some(HttpMethod::parse(&s)?),
        None => None,
    };
    Ok(RouteStub {
        route_name: name,
        path,
        method,
    })
}

fn looks_like_byfield_shorthand(name: &str) -> bool {
    name.contains("_by_")
        && !name.is_empty()
        && !name.starts_with("_by_")
        && !name.ends_with("_by_")
}

fn is_empty_body(v: &YamlValue) -> bool {
    matches!(v, YamlValue::Null) || matches!(v, YamlValue::Mapping(m) if m.is_empty())
}

/// A verbose byField route body `{entity, byField, methods?}` — recognised so it isn't mistaken for a
/// stub (the generated router mounts the actual routes). methods content is irrelevant to recognition.
fn is_verbose_byfield(v: &YamlValue) -> bool {
    let YamlValue::Mapping(map) = v else {
        return false;
    };
    let entity = map
        .get(YamlValue::String("entity".into()))
        .and_then(|x| x.as_str());
    let by_field = map
        .get(YamlValue::String("byField".into()))
        .and_then(|x| x.as_str());
    matches!((entity, by_field), (Some(e), Some(f)) if !e.is_empty() && !f.is_empty())
}

fn build_custom(name: String, fields: RawRouteFields) -> Result<CustomRouteSpec, RoutesError> {
    let path = fields
        .path
        .ok_or_else(|| RoutesError::Schema(format!("route '{}' missing `path`", name)))?;
    let method = HttpMethod::parse(
        fields
            .method
            .as_deref()
            .ok_or_else(|| RoutesError::Schema(format!("route '{}' missing `method`", name)))?,
    )?;
    let route_class = fields.route_class.unwrap();
    let module = fields
        .module
        .ok_or_else(|| RoutesError::Schema(format!("custom route '{}' missing `module`", name)))?;
    let args = match fields.args {
        None => Vec::new(),
        Some(items) => items
            .into_iter()
            .map(|raw| super::services::parse_arg(raw).map_err(RoutesError::Schema))
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(CustomRouteSpec {
        route_name: name,
        path,
        method,
        route_class,
        module,
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_yaml_returns_empty_doc() {
        let doc = parse_routes("").unwrap();
        assert!(doc.generic.is_empty());
        assert!(doc.custom.is_empty());
        assert!(doc.stubs.is_empty());
    }

    #[test]
    fn bare_string_byfield_entries_are_recognised_and_dropped() {
        // The generated router owns byField/CRUD; the runtime only accepts these forms so they don't fall into the build_stub "missing path" trap.
        let yaml = concat!(
            "routes:\n",
            "  - notifications_by_key\n",
            "  - get_notifications_by_notification_type\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert!(doc.generic.is_empty());
        assert!(doc.stubs.is_empty());
        assert!(doc.custom.is_empty());
    }

    #[test]
    fn map_with_null_or_empty_body_is_recognised_and_dropped() {
        // why: matches TS parseCrudRouteSpecs which accepts `{ token: null }` / `{ token: {} }` as the bare-string shorthand. YAML round-trips shed null↔{}, so both forms must parse without becoming stubs.
        let yaml = concat!(
            "routes:\n",
            "  - notifications_by_key: null\n",
            "  - delete_notifications_by_notification_type: ~\n",
            "  - notification_by_key: {}\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert!(doc.generic.is_empty());
        assert!(doc.stubs.is_empty());
        assert!(doc.custom.is_empty());
    }

    #[test]
    fn verbose_byfield_body_is_recognised_and_dropped() {
        // The verbose form `{entity, byField, methods?}` the codegen emits must be recognised (so it isn't a "missing path" stub); the generated router mounts the actual routes, so the runtime keeps nothing.
        let yaml = concat!(
            "routes:\n",
            "  - notification_by_key:\n",
            "      entity: notification\n",
            "      byField: key\n",
            "  - notification_by_type:\n",
            "      entity: notification\n",
            "      byField: notification_type\n",
            "      methods: [GET, DELETE]\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert!(doc.stubs.is_empty());
        assert!(doc.generic.is_empty());
        assert!(doc.custom.is_empty());
    }

    #[test]
    fn byfield_looking_stub_with_real_path_field_still_parses_as_stub() {
        // Regression guard: only an EMPTY body short-circuits to references — a fully-shaped stub (path + method, no service/routeClass) keeps its old meaning.
        let yaml = concat!(
            "routes:\n",
            "  - oauth_by_token:\n",
            "      path: /api/oauth/by-token\n",
            "      method: GET\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.stubs.len(), 1);
        assert_eq!(doc.stubs[0].route_name, "oauth_by_token");
        assert_eq!(doc.stubs[0].path, "/api/oauth/by-token");
    }

    #[test]
    fn stub_missing_path_with_byfield_looking_name_suggests_shorthand() {
        // Helpful error: when the user writes `notification_by_key:` followed by a fields-shaped body that's missing `path:` (and missing service/routeClass), the error nudges them toward the shorthand instead of just saying "missing path".
        let yaml = concat!(
            "routes:\n",
            "  - notifications_by_key:\n",
            "      method: GET\n",
        );
        let err = parse_routes(yaml).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("byField shorthand"),
            "expected byField shorthand hint, got: {}",
            msg
        );
    }

    #[test]
    fn stub_missing_path_with_normal_name_keeps_old_error() {
        let yaml = concat!("routes:\n", "  - oauth_test:\n", "      method: POST\n",);
        let err = parse_routes(yaml).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("missing `path`"));
        assert!(
            !msg.contains("byField shorthand"),
            "non-byField-looking names shouldn't get the shorthand hint: {}",
            msg
        );
    }

    #[test]
    fn parses_generic_route() {
        let yaml = concat!(
            "routes:\n",
            "  - import_contacts:\n",
            "      path: /api/contacts/import\n",
            "      method: POST\n",
            "      service: ContactImportService\n",
            "      serviceMethod: import\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.generic.len(), 1);
        let g = &doc.generic[0];
        assert_eq!(g.route_name, "import_contacts");
        assert_eq!(g.path, "/api/contacts/import");
        assert_eq!(g.method, HttpMethod::Post);
        assert_eq!(g.service, "ContactImportService");
        assert_eq!(g.service_method, "import");
    }

    #[test]
    fn parses_custom_route_with_args() {
        let yaml = concat!(
            "routes:\n",
            "  - my_handler:\n",
            "      path: /api/x\n",
            "      method: GET\n",
            "      routeClass: MyHandler\n",
            "      module: ./routes/my-handler\n",
            "      args:\n",
            "        - repo: notification\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.custom.len(), 1);
        let c = &doc.custom[0];
        assert_eq!(c.route_class, "MyHandler");
        assert_eq!(c.module, "./routes/my-handler");
        assert_eq!(c.args.len(), 1);
        match &c.args[0] {
            ArgSpec::Repo { name } => assert_eq!(name, "notification"),
            _ => panic!("expected Repo arg"),
        }
    }

    #[test]
    fn unknown_method_errors() {
        let yaml = concat!(
            "routes:\n",
            "  - bad:\n",
            "      path: /x\n",
            "      method: HACK\n",
            "      service: S\n",
            "      serviceMethod: m\n",
        );
        let err = parse_routes(yaml).unwrap_err();
        assert!(format!("{}", err).contains("HACK"));
    }

    #[test]
    fn route_without_service_or_routeclass_stores_as_stub() {
        let yaml = concat!(
            "routes:\n",
            "  - oauth_test:\n",
            "      path: /api/oauth/test\n",
            "      method: POST\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.stubs.len(), 1);
        assert_eq!(doc.stubs[0].route_name, "oauth_test");
        assert_eq!(doc.stubs[0].method, Some(HttpMethod::Post));
    }

    #[test]
    fn parses_view_type_routes_eager_write_path() {
        let yaml = concat!(
            "view_type_routes:\n",
            "  eager_write_path:\n",
            "    - todo.tasks\n",
            "    - user.posts.tags\n",
            "routes: []\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.eager_write_paths, vec!["todo.tasks", "user.posts.tags"]);
    }

    #[test]
    fn parses_eager_write_path_from_includes_directive() {
        let yaml = concat!(
            "includes:\n",
            "  - view_type_routes:\n",
            "      filter: type inherits datasource_types\n",
            "      eager_write_path:\n",
            "        - contact.addresses\n",
            "        - post.tags\n",
            "routes: []\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(
            doc.eager_write_paths,
            vec!["contact.addresses", "post.tags"]
        );
    }

    #[test]
    fn parses_eager_path_from_includes_directive() {
        let yaml = concat!(
            "includes:\n",
            "  - view_type_routes:\n",
            "      eager_path:\n",
            "        - contact.addresses\n",
            "        - contact_group.members\n",
            "      eager_write_path:\n",
            "        - contact.addresses\n",
            "routes: []\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(
            doc.eager_paths,
            vec!["contact.addresses", "contact_group.members"]
        );
        assert_eq!(doc.eager_write_paths, vec!["contact.addresses"]);
    }

    #[test]
    fn service_method_defaults_to_route_name() {
        let yaml = concat!(
            "routes:\n",
            "  - signin:\n",
            "      path: /api/signin\n",
            "      method: POST\n",
            "      service: AuthService\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.generic[0].service_method, "signin");
    }

    #[test]
    fn parses_combined_routes_with_direct_fk_children() {
        let yaml = concat!(
            "combined_routes:\n",
            "  - contact:\n",
            "      route: /api/contacts/{id}\n",
            "      combined_types:\n",
            "        - address:\n",
            "            route: /addresses\n",
            "        - phone:\n",
            "            route: /phones\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.combined.len(), 1);
        let entry = &doc.combined[0];
        assert_eq!(entry.parent, "contact");
        assert_eq!(entry.route, "/api/contacts/{id}");
        assert_eq!(entry.children.len(), 2);
        assert_eq!(entry.children[0].name, "address");
        assert_eq!(entry.children[0].route.as_deref(), Some("/addresses"));
        assert_eq!(entry.children[1].name, "phone");
        assert!(entry.children[0].via.is_none());
    }

    #[test]
    fn parses_combined_routes_with_m2m_via_target() {
        let yaml = concat!(
            "combined_routes:\n",
            "  - contact_group:\n",
            "      route: /api/contact-groups/{id}\n",
            "      combined_types:\n",
            "        - contact:\n",
            "            via: contact_group_member\n",
            "            target: contact\n",
            "            route: /members\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.combined.len(), 1);
        let entry = &doc.combined[0];
        assert_eq!(entry.parent, "contact_group");
        let child = &entry.children[0];
        assert_eq!(child.via.as_deref(), Some("contact_group_member"));
        assert_eq!(child.target.as_deref(), Some("contact"));
        assert_eq!(child.route.as_deref(), Some("/members"));
    }

    #[test]
    fn combined_routes_kebab_keys_get_normalized_to_snake() {
        let yaml = concat!(
            "combined_routes:\n",
            "  - contact-group:\n",
            "      route: /api/x/{id}\n",
            "      combined_types:\n",
            "        - some-child: {}\n",
        );
        let doc = parse_routes(yaml).unwrap();
        assert_eq!(doc.combined[0].parent, "contact_group");
        assert_eq!(doc.combined[0].children[0].name, "some_child");
    }

    #[test]
    fn combined_routes_string_child_uses_defaults() {
        let yaml = concat!(
            "combined_routes:\n",
            "  - contact:\n",
            "      route: /api/contacts/{id}\n",
            "      combined_types:\n",
            "        - address\n",
        );
        let doc = parse_routes(yaml).unwrap();
        let child = &doc.combined[0].children[0];
        assert_eq!(child.name, "address");
        assert!(child.via.is_none());
        assert!(child.target.is_none());
        assert!(child.route.is_none());
    }
}
