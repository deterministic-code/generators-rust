use std::collections::HashMap;

use serde_json::Value;

use super::datasource_types::DatasourceTypeDef;
use super::routes::RoutesDoc;
use super::view_types::{ViewTypeDef, ViewTypesDoc};

#[derive(Debug, Clone, PartialEq)]
pub enum BindingKind {
    DirectFk {
        fk_column: String,
    },
    M2m {
        junction_table: String,
        parent_fk_column: String,
        target_fk_column: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EagerWriteChildBinding {
    pub kind: BindingKind,
    pub field_name: String,
    pub child_table: String,
    pub children: Vec<EagerWriteChildBinding>,
    /// False when the view field is a single nested object (`datasource_types.address`).
    pub is_array: bool,
}

pub fn build_eager_write_bindings(
    routes_doc: &RoutesDoc,
    view_doc: &ViewTypesDoc,
    datasource_types: &[DatasourceTypeDef],
) -> HashMap<String, Vec<EagerWriteChildBinding>> {
    build_child_bindings(&routes_doc.eager_write_paths, view_doc, datasource_types)
}

/// Read-side eager bindings over the union of `eager_path` and `eager_write_path`: a child you can
/// embed on write must round-trip on a member read (eager-write ⟹ eager-read), so a write-embed
/// relation eager-loads even when `eager_path` omits it. Reuses the eager-write binding model — the
/// relation graph is identical; only the driver path list differs.
pub fn build_eager_read_bindings(
    routes_doc: &RoutesDoc,
    view_doc: &ViewTypesDoc,
    datasource_types: &[DatasourceTypeDef],
) -> HashMap<String, Vec<EagerWriteChildBinding>> {
    let mut paths = routes_doc.eager_paths.clone();
    for path in &routes_doc.eager_write_paths {
        if !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    build_child_bindings(&paths, view_doc, datasource_types)
}

fn build_child_bindings(
    paths: &[String],
    view_doc: &ViewTypesDoc,
    datasource_types: &[DatasourceTypeDef],
) -> HashMap<String, Vec<EagerWriteChildBinding>> {
    let mut out: HashMap<String, Vec<EagerWriteChildBinding>> = HashMap::new();
    if paths.is_empty() {
        return out;
    }

    let view_map: HashMap<&str, &ViewTypeDef> = view_doc
        .types
        .iter()
        .map(|v| (v.name.as_str(), v))
        .collect();
    let ds_map: HashMap<&str, &DatasourceTypeDef> = datasource_types
        .iter()
        .map(|d| (d.name.as_str(), d))
        .collect();

    // why nested-map shape: mirrors TS childrenByParent — paths that share an intermediate (user.posts + user.posts.tags) must merge under the same parent.fieldName so the binding tree dedupes correctly.
    let mut children_by_parent: HashMap<String, Vec<EagerWriteChildBinding>> = HashMap::new();

    for path in paths {
        let segments: Vec<&str> = path.split('.').collect();
        if segments.len() < 2 {
            continue;
        }
        let mut current_parent = segments[0].to_string();
        for seg in &segments[1..] {
            let field_name = seg.to_string();
            let entry = children_by_parent
                .entry(current_parent.clone())
                .or_default();
            let existing_idx = entry.iter().position(|b| b.field_name == field_name);
            let next_parent = if let Some(idx) = existing_idx {
                entry[idx].child_table.clone()
            } else {
                let Some(built) =
                    build_one_binding(&current_parent, &field_name, &view_map, &ds_map)
                else {
                    break;
                };
                let next = built.child_table.clone();
                entry.push(built);
                next
            };
            current_parent = next_parent;
        }
    }

    // why two-pass: attach nested grandchildren by re-traversing the map; mirrors TS attachChildrenFor recursion.
    for root in view_map.keys() {
        let bindings = attach_children_for(root, &children_by_parent);
        if !bindings.is_empty() {
            out.insert(root.to_string(), bindings);
        }
    }

    out
}

fn attach_children_for(
    view_name: &str,
    children_by_parent: &HashMap<String, Vec<EagerWriteChildBinding>>,
) -> Vec<EagerWriteChildBinding> {
    let Some(bucket) = children_by_parent.get(view_name) else {
        return Vec::new();
    };
    bucket
        .iter()
        .map(|b| EagerWriteChildBinding {
            kind: b.kind.clone(),
            field_name: b.field_name.clone(),
            child_table: b.child_table.clone(),
            children: attach_children_for(&b.child_table, children_by_parent),
            is_array: b.is_array,
        })
        .collect()
}

fn build_one_binding(
    parent_view_name: &str,
    child_field_name: &str,
    view_map: &HashMap<&str, &ViewTypeDef>,
    ds_map: &HashMap<&str, &DatasourceTypeDef>,
) -> Option<EagerWriteChildBinding> {
    let parent_view = view_map.get(parent_view_name)?;
    let field = parent_view
        .fields
        .iter()
        .find(|f| f.name == child_field_name)?;
    let field_type = field.r#type.as_str();
    let references = field.references.as_deref()?;

    let (element, is_array) = parse_relation_element_type(field_type)?;
    let (ref_table, ref_column) = parse_reference(references)?;

    if ref_table == element {
        return Some(EagerWriteChildBinding {
            kind: BindingKind::DirectFk {
                fk_column: ref_column.to_string(),
            },
            field_name: child_field_name.to_string(),
            child_table: element.to_string(),
            children: Vec::new(),
            is_array,
        });
    }

    let junction = ds_map.get(ref_table)?;
    if junction.datasource_type.as_deref() != Some("many-to-many") {
        return None;
    }
    let target_fk_field = junction.fields.iter().find(|f| {
        if f.name == ref_column {
            return false;
        }
        let Some(refs) = &f.references else {
            return false;
        };
        let Some((target_table, _)) = parse_reference(refs) else {
            return false;
        };
        target_table == element
    })?;

    Some(EagerWriteChildBinding {
        kind: BindingKind::M2m {
            junction_table: ref_table.to_string(),
            parent_fk_column: ref_column.to_string(),
            target_fk_column: target_fk_field.name.clone(),
        },
        field_name: child_field_name.to_string(),
        child_table: element.to_string(),
        children: Vec::new(),
        is_array,
    })
}

fn parse_relation_element_type(t: &str) -> Option<(&str, bool)> {
    let rest = t.strip_prefix("datasource_types.")?;
    if rest.is_empty() || rest.contains('.') {
        return None;
    }
    if let Some(elem) = rest.strip_suffix("[]") {
        if elem.is_empty() || elem.contains('.') {
            return None;
        }
        Some((elem, true))
    } else {
        Some((rest, false))
    }
}

fn parse_reference(r: &str) -> Option<(&str, &str)> {
    let rest = r.strip_prefix("datasource_types.").unwrap_or(r);
    let (table, column) = rest.split_once('.')?;
    if table.is_empty() || column.is_empty() {
        return None;
    }
    Some((table, column))
}

impl EagerWriteChildBinding {
    pub fn pack_children(&self, rows: Vec<Value>) -> Value {
        if self.is_array {
            Value::Array(rows)
        } else {
            rows.into_iter().next().unwrap_or(Value::Null)
        }
    }

    pub fn unpack_incoming(&self, value: Option<&Value>) -> Option<Vec<Value>> {
        match (self.is_array, value) {
            (true, Some(Value::Array(arr))) => Some(arr.clone()),
            (true, _) => None,
            (false, None) => None,
            (false, Some(Value::Null)) => Some(Vec::new()),
            (false, Some(obj @ Value::Object(_))) => Some(vec![obj.clone()]),
            (false, _) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{parse_datasource_types, parse_routes, parse_view_types};

    fn load_demo() -> (RoutesDoc, ViewTypesDoc, Vec<DatasourceTypeDef>) {
        // Self-contained fixture mirroring samples/demo-backend relations the
        // eager-write tests assert: contact→address/phone, todo→task/meeting,
        // user→post→tag (M2M via post_tag).
        let routes = parse_routes(concat!(
            "includes:\n",
            "  - view_type_routes:\n",
            "      eager_path:\n",
            "        - user.posts.tags\n",
            "        - todo.tasks\n",
            "        - todo.meetings\n",
            "        - post.tags\n",
            "      eager_write_path:\n",
            "        - contact.addresses\n",
            "        - contact.phones\n",
            "        - todo.tasks\n",
            "        - todo.meetings\n",
            "        - user.posts\n",
            "        - user.posts.tags\n",
            "        - post.tags\n",
            "routes: []\n",
        ))
        .unwrap();
        let views = parse_view_types(concat!(
            "types:\n",
            "  - user:\n",
            "      inherits: datasource_types.user\n",
            "      fields:\n",
            "        - posts:\n",
            "            type: datasource_types.post[]\n",
            "            references: datasource_types.post.author_id\n",
            "  - post:\n",
            "      inherits: datasource_types.post\n",
            "      fields:\n",
            "        - tags:\n",
            "            type: datasource_types.tag[]\n",
            "            references: datasource_types.post_tag.post_id\n",
            "  - todo:\n",
            "      inherits: datasource_types.todo\n",
            "      fields:\n",
            "        - tasks:\n",
            "            type: datasource_types.task[]\n",
            "            references: datasource_types.task.todo_id\n",
            "        - meetings:\n",
            "            type: datasource_types.meeting[]\n",
            "            references: datasource_types.meeting.todo_id\n",
            "  - contact:\n",
            "      inherits: datasource_types.contact\n",
            "      fields:\n",
            "        - addresses:\n",
            "            type: datasource_types.address[]\n",
            "            references: datasource_types.address.contact_id\n",
            "        - phones:\n",
            "            type: datasource_types.phone[]\n",
            "            references: datasource_types.phone.contact_id\n",
        ))
        .unwrap();
        let ds = parse_datasource_types(concat!(
            "types:\n",
            "  - user:\n",
            "      fields:\n",
            "        - username:\n",
            "            type: string\n",
            "  - post:\n",
            "      fields:\n",
            "        - author_id:\n",
            "            type: number\n",
            "            references: user.id\n",
            "        - title:\n",
            "            type: string\n",
            "  - tag:\n",
            "      fields:\n",
            "        - name:\n",
            "            type: string\n",
            "  - post_tag:\n",
            "      datasource_type: many-to-many\n",
            "      fields:\n",
            "        - post_id:\n",
            "            type: number\n",
            "            references: post.id\n",
            "        - tag_id:\n",
            "            type: number\n",
            "            references: tag.id\n",
            "  - todo:\n",
            "      fields:\n",
            "        - title:\n",
            "            type: string\n",
            "  - task:\n",
            "      fields:\n",
            "        - todo_id:\n",
            "            type: number\n",
            "            references: todo.id\n",
            "        - description:\n",
            "            type: string\n",
            "  - meeting:\n",
            "      fields:\n",
            "        - todo_id:\n",
            "            type: number\n",
            "            references: todo.id\n",
            "  - contact:\n",
            "      fields:\n",
            "        - name:\n",
            "            type: string\n",
            "  - address:\n",
            "      fields:\n",
            "        - contact_id:\n",
            "            type: number\n",
            "            references: contact.id\n",
            "        - line1:\n",
            "            type: string\n",
            "  - phone:\n",
            "      fields:\n",
            "        - contact_id:\n",
            "            type: number\n",
            "            references: contact.id\n",
            "        - number:\n",
            "            type: string\n",
        ))
        .unwrap();
        (routes, views, ds.types)
    }

    #[test]
    fn parse_reference_extracts_table_and_column() {
        assert_eq!(
            parse_reference("datasource_types.task.todo_id"),
            Some(("task", "todo_id"))
        );
        assert_eq!(parse_reference("task.todo_id"), Some(("task", "todo_id")));
    }

    #[test]
    fn parse_relation_element_type_accepts_array_and_singular() {
        assert_eq!(
            parse_relation_element_type("datasource_types.task[]"),
            Some(("task", true))
        );
        assert_eq!(
            parse_relation_element_type("datasource_types.address"),
            Some(("address", false))
        );
        assert_eq!(parse_relation_element_type("task[]"), None);
        assert_eq!(
            parse_relation_element_type("datasource_types.address.contact_id"),
            None
        );
    }

    #[test]
    fn pack_unpack_singular_object() {
        let b = EagerWriteChildBinding {
            kind: BindingKind::DirectFk {
                fk_column: "contact_id".to_string(),
            },
            field_name: "address".to_string(),
            child_table: "address".to_string(),
            children: Vec::new(),
            is_array: false,
        };
        let obj = serde_json::json!({ "line1": "x" });
        assert_eq!(b.unpack_incoming(Some(&obj)), Some(vec![obj.clone()]));
        assert_eq!(b.unpack_incoming(Some(&Value::Null)), Some(vec![]));
        assert_eq!(b.pack_children(vec![obj.clone()]), obj);
        assert_eq!(b.pack_children(vec![]), Value::Null);
    }

    #[test]
    fn pack_unpack_collection_array() {
        let b = EagerWriteChildBinding {
            kind: BindingKind::DirectFk {
                fk_column: "contact_id".to_string(),
            },
            field_name: "addresses".to_string(),
            child_table: "address".to_string(),
            children: Vec::new(),
            is_array: true,
        };
        let obj = serde_json::json!({ "line1": "x" });
        let arr = Value::Array(vec![obj.clone()]);
        assert_eq!(b.unpack_incoming(Some(&arr)), Some(vec![obj.clone()]));
        assert_eq!(b.unpack_incoming(Some(&obj)), None);
        assert_eq!(b.unpack_incoming(Some(&Value::Null)), None);
        assert_eq!(b.unpack_incoming(None), None);
        assert_eq!(b.pack_children(vec![obj.clone()]), arr);
        assert_eq!(b.pack_children(vec![]), Value::Array(vec![]));
        let singular = EagerWriteChildBinding {
            is_array: false,
            ..b.clone()
        };
        assert_eq!(singular.unpack_incoming(Some(&arr)), None);
        assert_eq!(singular.unpack_incoming(None), None);
    }

    #[test]
    fn empty_paths_returns_empty_map() {
        let routes = RoutesDoc::default();
        let views = ViewTypesDoc::default();
        let map = build_eager_write_bindings(&routes, &views, &[]);
        assert!(map.is_empty());
    }

    #[test]
    fn build_eager_read_bindings_unions_eager_path_and_eager_write_path() {
        let (_routes, views, ds) = load_demo();

        // eager_path drives read bindings.
        let from_read_path = RoutesDoc {
            eager_paths: vec![
                "contact.addresses".to_string(),
                "contact.phones".to_string(),
            ],
            ..Default::default()
        };
        let map = build_eager_read_bindings(&from_read_path, &views, &ds);
        let contact = map
            .get("contact")
            .expect("contact read bindings from eager_path");
        assert_eq!(contact.len(), 2);
        assert!(build_eager_write_bindings(&from_read_path, &views, &ds).is_empty());

        // eager_write_path alone (contact absent from eager_path) still yields read bindings — a
        // write-embed child must round-trip on read.
        let from_write_path = RoutesDoc {
            eager_write_paths: vec![
                "contact.addresses".to_string(),
                "contact.phones".to_string(),
            ],
            ..Default::default()
        };
        let map = build_eager_read_bindings(&from_write_path, &views, &ds);
        let contact = map
            .get("contact")
            .expect("contact read bindings from eager_write_path");
        assert!(contact.iter().any(|b| b.field_name == "addresses"));
        assert!(contact.iter().any(|b| b.field_name == "phones"));
    }

    #[test]
    fn demo_backend_todo_has_direct_fk_tasks_and_meetings() {
        let (routes, views, ds) = load_demo();
        let map = build_eager_write_bindings(&routes, &views, &ds);
        let todo_bindings = map.get("todo").expect("todo bindings");
        assert_eq!(todo_bindings.len(), 2);
        let tasks = todo_bindings
            .iter()
            .find(|b| b.field_name == "tasks")
            .unwrap();
        assert_eq!(tasks.child_table, "task");
        assert!(matches!(
            &tasks.kind,
            BindingKind::DirectFk { fk_column } if fk_column == "todo_id"
        ));
        assert!(tasks.children.is_empty());
        let meetings = todo_bindings
            .iter()
            .find(|b| b.field_name == "meetings")
            .unwrap();
        assert_eq!(meetings.child_table, "meeting");
        assert!(matches!(
            &meetings.kind,
            BindingKind::DirectFk { fk_column } if fk_column == "todo_id"
        ));
    }

    #[test]
    fn demo_backend_contact_has_addresses_and_phones() {
        let (routes, views, ds) = load_demo();
        let map = build_eager_write_bindings(&routes, &views, &ds);
        let contact_bindings = map.get("contact").expect("contact bindings");
        assert_eq!(contact_bindings.len(), 2);
        let addrs = contact_bindings
            .iter()
            .find(|b| b.field_name == "addresses")
            .unwrap();
        assert_eq!(addrs.child_table, "address");
        assert!(matches!(
            &addrs.kind,
            BindingKind::DirectFk { fk_column } if fk_column == "contact_id"
        ));
        let phones = contact_bindings
            .iter()
            .find(|b| b.field_name == "phones")
            .unwrap();
        assert_eq!(phones.child_table, "phone");
        assert!(matches!(
            &phones.kind,
            BindingKind::DirectFk { fk_column } if fk_column == "contact_id"
        ));
    }

    #[test]
    fn demo_backend_user_has_posts_with_nested_tags_m2m() {
        let (routes, views, ds) = load_demo();
        let map = build_eager_write_bindings(&routes, &views, &ds);
        let user_bindings = map.get("user").expect("user bindings");
        assert_eq!(user_bindings.len(), 1);
        let posts = &user_bindings[0];
        assert_eq!(posts.field_name, "posts");
        assert_eq!(posts.child_table, "post");
        assert!(matches!(
            &posts.kind,
            BindingKind::DirectFk { fk_column } if fk_column == "author_id"
        ));
        assert_eq!(posts.children.len(), 1);
        let tags = &posts.children[0];
        assert_eq!(tags.field_name, "tags");
        assert_eq!(tags.child_table, "tag");
        match &tags.kind {
            BindingKind::M2m {
                junction_table,
                parent_fk_column,
                target_fk_column,
            } => {
                assert_eq!(junction_table, "post_tag");
                assert_eq!(parent_fk_column, "post_id");
                assert_eq!(target_fk_column, "tag_id");
            }
            other => panic!("expected M2m, got {:?}", other),
        }
    }

    #[test]
    fn demo_backend_post_has_tags_m2m() {
        let (routes, views, ds) = load_demo();
        let map = build_eager_write_bindings(&routes, &views, &ds);
        let post_bindings = map.get("post").expect("post bindings");
        assert_eq!(post_bindings.len(), 1);
        let tags = &post_bindings[0];
        assert_eq!(tags.field_name, "tags");
        assert_eq!(tags.child_table, "tag");
        match &tags.kind {
            BindingKind::M2m {
                junction_table,
                parent_fk_column,
                target_fk_column,
            } => {
                assert_eq!(junction_table, "post_tag");
                assert_eq!(parent_fk_column, "post_id");
                assert_eq!(target_fk_column, "tag_id");
            }
            other => panic!("expected M2m, got {:?}", other),
        }
    }

    #[test]
    fn singular_nested_object_is_direct_fk() {
        let routes = parse_routes(concat!(
            "includes:\n",
            "  - view_type_routes:\n",
            "      eager_write_path:\n",
            "        - contact.address\n",
            "      eager_path:\n",
            "        - contact.address\n",
            "routes: []\n",
        ))
        .unwrap();
        let views = parse_view_types(concat!(
            "types:\n",
            "  - contact:\n",
            "      inherits: datasource_types.contact\n",
            "      fields:\n",
            "        - address:\n",
            "            type: datasource_types.address\n",
            "            references: datasource_types.address.contact_id\n",
        ))
        .unwrap();
        let ds = parse_datasource_types(concat!(
            "types:\n",
            "  - contact:\n",
            "      fields:\n",
            "        - name:\n",
            "            type: string\n",
            "  - address:\n",
            "      fields:\n",
            "        - contact_id:\n",
            "            type: number\n",
            "            references: contact.id\n",
            "        - line1:\n",
            "            type: string\n",
        ))
        .unwrap();
        let map = build_eager_write_bindings(&routes, &views, &ds.types);
        let contact = map.get("contact").expect("contact bindings");
        assert_eq!(contact.len(), 1);
        assert_eq!(contact[0].field_name, "address");
        assert_eq!(contact[0].child_table, "address");
        assert!(!contact[0].is_array);
        assert!(matches!(
            &contact[0].kind,
            BindingKind::DirectFk { fk_column } if fk_column == "contact_id"
        ));
        let read = build_eager_read_bindings(&routes, &views, &ds.types);
        assert!(!read.get("contact").unwrap()[0].is_array);
    }
}
