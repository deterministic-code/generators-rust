use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use deterministic::error::RepositoryError;
use deterministic::repositories::RowMap;
use deterministic::routes::{
    create_nested_many_to_many_router, CrudService, IdType, LinkVerb, NestedManyToManyRouterConfig,
    ValidatorFn,
};

#[derive(Default)]
struct JunctionService {
    rows: Mutex<Vec<RowMap>>,
    next_id: Mutex<i64>,
}

impl JunctionService {
    fn new() -> Self {
        Self {
            rows: Mutex::new(Vec::new()),
            next_id: Mutex::new(1),
        }
    }
    fn seed(&self, rows: Vec<RowMap>) {
        *self.rows.lock().unwrap() = rows;
    }
    fn all(&self) -> Vec<RowMap> {
        self.rows.lock().unwrap().clone()
    }
}

fn junction_row(id: i64, user_id: i64, role_id: i64) -> RowMap {
    let mut r: BTreeMap<String, Value> = BTreeMap::new();
    r.insert("id".to_string(), Value::from(id));
    r.insert("user_id".to_string(), Value::from(user_id));
    r.insert("role_id".to_string(), Value::from(role_id));
    r
}

#[async_trait]
impl CrudService for JunctionService {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let rows = self.rows.lock().unwrap();
        Ok(rows.iter().find(|r| r.get("id") == Some(id)).cloned())
    }
    async fn add(&self, body: RowMap) -> Result<RowMap, RepositoryError> {
        let mut row = body;
        let mut next = self.next_id.lock().unwrap();
        let id = *next;
        *next += 1;
        row.insert("id".to_string(), Value::from(id));
        self.rows.lock().unwrap().push(row.clone());
        Ok(row)
    }
    async fn update(
        &self,
        id: &Value,
        body: RowMap,
        _expected: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        let mut rows = self.rows.lock().unwrap();
        let Some(row) = rows.iter_mut().find(|r| r.get("id") == Some(id)) else {
            return Ok(None);
        };
        for (k, v) in body {
            row.insert(k, v);
        }
        Ok(Some(row.clone()))
    }
    async fn delete(&self, id: &Value, _expected: Option<&str>) -> Result<bool, RepositoryError> {
        let mut rows = self.rows.lock().unwrap();
        let Some(idx) = rows.iter().position(|r| r.get("id") == Some(id)) else {
            return Ok(false);
        };
        rows.remove(idx);
        Ok(true)
    }
}

struct BuildOpts {
    create_validator: Option<ValidatorFn>,
    child_create_validator: Option<ValidatorFn>,
    patch_child_validator: Option<ValidatorFn>,
    child_service: Option<Arc<dyn CrudService>>,
    link_verb: LinkVerb,
    child_parent_fk_field: Option<String>,
}

impl Default for BuildOpts {
    fn default() -> Self {
        Self {
            create_validator: None,
            child_create_validator: None,
            patch_child_validator: None,
            child_service: None,
            link_verb: LinkVerb::default(),
            child_parent_fk_field: None,
        }
    }
}

fn make_router(svc: Arc<JunctionService>, opts: BuildOpts) -> Router {
    create_nested_many_to_many_router(NestedManyToManyRouterConfig {
        junction_service: svc,
        child_service: opts.child_service,
        parent_entity_name: "User".to_string(),
        child_entity_name: "Role".to_string(),
        base_path: "/api/users/{user_id}/roles".to_string(),
        parent_param_name: "user_id".to_string(),
        child_param_name: "role_id".to_string(),
        parent_fk_field: "user_id".to_string(),
        child_fk_field: "role_id".to_string(),
        parent_id_type: IdType::Integer,
        child_id_type: IdType::Integer,
        create_validator: opts.create_validator,
        child_create_validator: opts.child_create_validator,
        patch_child_validator: opts.patch_child_validator,
        coerce_row: None,
        link_verb: opts.link_verb,
        child_parent_fk_field: opts.child_parent_fk_field,
    })
}

fn custom_router() -> Router {
    Router::new().route(
        "/api/custom-thing",
        get(|| async { axum::Json(json!({ "custom": "yes" })) }),
    )
}

async fn send(app: Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let req_body = match body {
        Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
        None => Body::empty(),
    };
    let response = app.oneshot(builder.body(req_body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let out: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, out)
}

fn assert_error_envelope(body: &Value, expected_code: &str) {
    let errors = body.get("errors").and_then(|v| v.as_array()).unwrap();
    assert_eq!(errors.len(), 1, "body: {}", body);
    assert_eq!(
        errors[0].get("code").unwrap().as_str().unwrap(),
        expected_code,
        "body: {}",
        body,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn list_filters_junction_rows_by_parent_fk() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![
        junction_row(1, 1, 10),
        junction_row(2, 2, 10),
        junction_row(3, 1, 11),
    ]);
    let router = make_router(svc, BuildOpts::default());
    let (status, body) = send(router, Method::GET, "/api/users/1/roles", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item.get("user_id").unwrap(), &json!(1));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn list_returns_empty_items_when_no_junction_row_matches_parent() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 2, 10)]);
    let router = make_router(svc, BuildOpts::default());
    let (status, body) = send(router, Method::GET, "/api/users/1/roles", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "items": [] }));
}

#[tokio::test(flavor = "current_thread")]
async fn list_returns_400_when_parent_id_is_invalid() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(svc, BuildOpts::default());
    for path in [
        "/api/users/0/roles",
        "/api/users/-1/roles",
        "/api/users/abc/roles",
    ] {
        let (status, body) = send(router.clone(), Method::GET, path, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error_envelope(&body, "VALIDATION_ERROR");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn post_creates_junction_with_parent_fk_auto_injected() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(svc.clone(), BuildOpts::default());
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/7/roles",
        Some(json!({ "role_id": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("user_id").unwrap(), &json!(7));
    assert_eq!(body.get("role_id").unwrap(), &json!(42));
    let stored = svc.all();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].get("user_id").unwrap(), &json!(7));
}

#[tokio::test(flavor = "current_thread")]
async fn post_overrides_client_supplied_parent_fk_with_url_value() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(svc, BuildOpts::default());
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/7/roles",
        Some(json!({ "user_id": 999, "role_id": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("user_id").unwrap(), &json!(7));
}

#[tokio::test(flavor = "current_thread")]
async fn post_returns_400_when_parent_id_is_invalid() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(svc, BuildOpts::default());
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/abc/roles",
        Some(json!({ "role_id": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
}

#[tokio::test(flavor = "current_thread")]
async fn post_create_validator_gates_body_with_400() {
    let hook: ValidatorFn = Arc::new(|body: &RowMap| {
        if body.get("role_id").is_some() {
            Ok(())
        } else {
            Err(vec!["role_id: required".to_string()])
        }
    });
    let svc = Arc::new(JunctionService::new());
    let router = make_router(
        svc,
        BuildOpts {
            create_validator: Some(hook),
            ..BuildOpts::default()
        },
    );
    let (status, body) = send(router, Method::POST, "/api/users/1/roles", Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
}

#[tokio::test(flavor = "current_thread")]
async fn delete_removes_the_junction_row_that_matches_both_fks() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![
        junction_row(1, 1, 10),
        junction_row(2, 1, 11),
        junction_row(3, 2, 10),
    ]);
    let router = make_router(svc.clone(), BuildOpts::default());
    let (status, body) = send(router, Method::DELETE, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "success": true }));
    let remaining = svc.all();
    assert_eq!(remaining.len(), 2);
    for row in &remaining {
        assert_ne!(
            (row.get("user_id").unwrap(), row.get("role_id").unwrap()),
            (&json!(1), &json!(10)),
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn delete_returns_404_when_no_junction_row_links_parent_and_child() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 2, 10)]);
    let router = make_router(svc, BuildOpts::default());
    let (status, body) = send(router, Method::DELETE, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn delete_returns_400_when_parent_id_is_invalid() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(svc, BuildOpts::default());
    let (status, body) = send(router, Method::DELETE, "/api/users/-1/roles/10", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("user_id"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn delete_returns_400_when_child_id_is_invalid() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(svc, BuildOpts::default());
    let (status, body) = send(router, Method::DELETE, "/api/users/1/roles/xyz", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("role_id"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn custom_route_still_works_when_merged_with_nested_m2m_router() {
    let svc = Arc::new(JunctionService::new());
    let router = Router::new()
        .merge(make_router(svc, BuildOpts::default()))
        .merge(custom_router());
    let (status, body) = send(router, Method::GET, "/api/custom-thing", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "custom": "yes" }));
}

#[derive(Default)]
struct ChildStore {
    rows: Mutex<Vec<RowMap>>,
    next_id: Mutex<i64>,
}

impl ChildStore {
    fn with_rows(rows: Vec<RowMap>) -> Arc<Self> {
        let seed_max = rows
            .iter()
            .filter_map(|r| r.get("id").and_then(|v| v.as_i64()))
            .max()
            .unwrap_or(0);
        Arc::new(Self {
            rows: Mutex::new(rows),
            next_id: Mutex::new(seed_max + 1),
        })
    }
    fn all(&self) -> Vec<RowMap> {
        self.rows.lock().unwrap().clone()
    }
}

fn child_row(id: i64, name: &str) -> RowMap {
    let mut r: BTreeMap<String, Value> = BTreeMap::new();
    r.insert("id".to_string(), Value::from(id));
    r.insert("name".to_string(), Value::String(name.to_string()));
    r
}

#[async_trait]
impl CrudService for ChildStore {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        Ok(self.rows.lock().unwrap().clone())
    }
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.get("id") == Some(id))
            .cloned())
    }
    async fn add(&self, mut body: RowMap) -> Result<RowMap, RepositoryError> {
        if !body.contains_key("id") {
            let mut next = self.next_id.lock().unwrap();
            let id = *next;
            *next += 1;
            body.insert("id".to_string(), Value::from(id));
        }
        self.rows.lock().unwrap().push(body.clone());
        Ok(body)
    }
    async fn update(
        &self,
        id: &Value,
        body: RowMap,
        _expected: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        let mut rows = self.rows.lock().unwrap();
        let Some(row) = rows.iter_mut().find(|r| r.get("id") == Some(id)) else {
            return Ok(None);
        };
        for (k, v) in body {
            row.insert(k, v);
        }
        Ok(Some(row.clone()))
    }
    async fn delete(&self, _id: &Value, _expected: Option<&str>) -> Result<bool, RepositoryError> {
        Ok(false)
    }
}

fn opts_with_child_service(store: Arc<ChildStore>, link_verb: LinkVerb) -> BuildOpts {
    BuildOpts {
        child_service: Some(store),
        link_verb,
        ..BuildOpts::default()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn link_route_absent_when_child_service_is_none() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(svc, BuildOpts::default());
    let (put_status, _) = send(router.clone(), Method::PUT, "/api/users/1/roles/10", None).await;
    assert_eq!(put_status, StatusCode::METHOD_NOT_ALLOWED);
    let (post_status, _) = send(router, Method::POST, "/api/users/1/roles/10", None).await;
    assert_eq!(post_status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn link_put_returns_200_creates_junction_and_returns_child_row() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc.clone(),
        opts_with_child_service(children, LinkVerb::Put),
    );
    let (status, body) = send(router, Method::PUT, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(10));
    assert_eq!(body.get("name").unwrap(), &json!("admin"));
    let stored = svc.all();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].get("user_id").unwrap(), &json!(1));
    assert_eq!(stored[0].get("role_id").unwrap(), &json!(10));
}

#[tokio::test(flavor = "current_thread")]
async fn link_post_returns_201_creates_junction_and_returns_child_row() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc.clone(),
        opts_with_child_service(children, LinkVerb::Post),
    );
    let (status, body) = send(router, Method::POST, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("id").unwrap(), &json!(10));
    let stored = svc.all();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].get("user_id").unwrap(), &json!(1));
    assert_eq!(stored[0].get("role_id").unwrap(), &json!(10));
}

#[tokio::test(flavor = "current_thread")]
async fn link_wrong_verb_returns_405_when_link_verb_is_put() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(svc, opts_with_child_service(children, LinkVerb::Put));
    let (status, _) = send(router, Method::POST, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn link_wrong_verb_returns_405_when_link_verb_is_post() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(svc, opts_with_child_service(children, LinkVerb::Post));
    let (status, _) = send(router, Method::PUT, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn link_is_idempotent_and_does_not_create_duplicate_junction() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 1, 10)]);
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc.clone(),
        opts_with_child_service(children, LinkVerb::Put),
    );
    let (status, body) = send(router, Method::PUT, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(10));
    let stored = svc.all();
    assert_eq!(stored.len(), 1, "junction was duplicated");
}

#[tokio::test(flavor = "current_thread")]
async fn link_returns_404_when_child_does_not_exist_in_child_service() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![]);
    let router = make_router(
        svc.clone(),
        opts_with_child_service(children, LinkVerb::Put),
    );
    let (status, body) = send(router, Method::PUT, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
    let stored = svc.all();
    assert!(
        stored.is_empty(),
        "no junction should be created when child is missing"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn link_delete_and_relink_creates_a_fresh_junction() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc.clone(),
        opts_with_child_service(children, LinkVerb::Put),
    );
    let (link_status, _) = send(router.clone(), Method::PUT, "/api/users/1/roles/10", None).await;
    assert_eq!(link_status, StatusCode::OK);
    assert_eq!(svc.all().len(), 1);
    let (del_status, _) = send(
        router.clone(),
        Method::DELETE,
        "/api/users/1/roles/10",
        None,
    )
    .await;
    assert_eq!(del_status, StatusCode::OK);
    assert!(svc.all().is_empty());
    let (relink_status, _) = send(router, Method::PUT, "/api/users/1/roles/10", None).await;
    assert_eq!(relink_status, StatusCode::OK);
    assert_eq!(svc.all().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn get_child_route_absent_when_child_service_is_none() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 1, 10)]);
    let router = make_router(svc, BuildOpts::default());
    let (status, _) = send(router, Method::GET, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn get_child_returns_child_row_when_junction_exists() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 1, 10)]);
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(svc, opts_with_child_service(children, LinkVerb::Put));
    let (status, body) = send(router, Method::GET, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(10));
    assert_eq!(body.get("name").unwrap(), &json!("admin"));
    assert!(
        body.get("user_id").is_none(),
        "should return child row, not junction row"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn get_child_returns_404_when_no_junction_links_parent_and_child() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(svc, opts_with_child_service(children, LinkVerb::Put));
    let (status, body) = send(router, Method::GET, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn get_child_returns_404_when_junction_exists_but_child_missing_from_child_service() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 1, 10)]);
    let children = ChildStore::with_rows(vec![]);
    let router = make_router(svc, opts_with_child_service(children, LinkVerb::Put));
    let (status, body) = send(router, Method::GET, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn get_child_returns_404_when_junction_belongs_to_different_parent() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 2, 10)]);
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(svc, opts_with_child_service(children, LinkVerb::Put));
    let (status, _) = send(router, Method::GET, "/api/users/1/roles/10", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "current_thread")]
async fn get_child_returns_400_when_child_id_is_invalid() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(svc, opts_with_child_service(children, LinkVerb::Put));
    let (status, body) = send(router, Method::GET, "/api/users/1/roles/xyz", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("role_id"), "message: {}", msg);
}

fn require_non_empty_name_validator() -> ValidatorFn {
    Arc::new(|body: &RowMap| {
        if body
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            Err(vec!["name: required".to_string()])
        } else {
            Ok(())
        }
    })
}

fn opts_with_child_service_and_patch(store: Arc<ChildStore>, patch: ValidatorFn) -> BuildOpts {
    BuildOpts {
        child_service: Some(store),
        patch_child_validator: Some(patch),
        ..BuildOpts::default()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn patch_child_route_absent_when_child_service_is_none() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(svc, BuildOpts::default());
    let (status, _) = send(
        router,
        Method::PATCH,
        "/api/users/1/roles/10",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn patch_child_route_absent_when_patch_validator_is_none_even_with_child_service() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(svc, opts_with_child_service(children, LinkVerb::Put));
    let (status, _) = send(
        router,
        Method::PATCH,
        "/api/users/1/roles/10",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn patch_child_updates_child_row_and_returns_updated_body() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 1, 10)]);
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc,
        opts_with_child_service_and_patch(children.clone(), require_non_empty_name_validator()),
    );
    let (status, body) = send(
        router,
        Method::PATCH,
        "/api/users/1/roles/10",
        Some(json!({ "name": "renamed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(10));
    assert_eq!(body.get("name").unwrap(), &json!("renamed"));
    let stored = children.find(&Value::from(10i64)).await.unwrap().unwrap();
    assert_eq!(stored.get("name").unwrap(), &json!("renamed"));
}

#[tokio::test(flavor = "current_thread")]
async fn patch_child_returns_404_when_no_junction_links_parent_and_child() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc,
        opts_with_child_service_and_patch(children, require_non_empty_name_validator()),
    );
    let (status, body) = send(
        router,
        Method::PATCH,
        "/api/users/1/roles/10",
        Some(json!({ "name": "renamed" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn patch_child_returns_404_when_junction_exists_but_child_missing() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 1, 10)]);
    let children = ChildStore::with_rows(vec![]);
    let router = make_router(
        svc,
        opts_with_child_service_and_patch(children, require_non_empty_name_validator()),
    );
    let (status, body) = send(
        router,
        Method::PATCH,
        "/api/users/1/roles/10",
        Some(json!({ "name": "renamed" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn patch_child_returns_400_when_body_fails_validator() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 1, 10)]);
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc,
        opts_with_child_service_and_patch(children.clone(), require_non_empty_name_validator()),
    );
    let (status, body) = send(
        router,
        Method::PATCH,
        "/api/users/1/roles/10",
        Some(json!({ "name": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    let stored = children.find(&Value::from(10i64)).await.unwrap().unwrap();
    assert_eq!(
        stored.get("name").unwrap(),
        &json!("admin"),
        "child must not be updated when validator rejects"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn patch_child_returns_404_when_junction_belongs_to_different_parent() {
    let svc = Arc::new(JunctionService::new());
    svc.seed(vec![junction_row(1, 2, 10)]);
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc,
        opts_with_child_service_and_patch(children, require_non_empty_name_validator()),
    );
    let (status, _) = send(
        router,
        Method::PATCH,
        "/api/users/1/roles/10",
        Some(json!({ "name": "renamed" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

fn opts_create_and_link(
    store: Arc<ChildStore>,
    child_create_validator: ValidatorFn,
    child_parent_fk_field: Option<String>,
) -> BuildOpts {
    BuildOpts {
        child_service: Some(store),
        child_create_validator: Some(child_create_validator),
        child_parent_fk_field,
        ..BuildOpts::default()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn create_and_link_creates_child_and_junction_and_returns_child_at_201() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![]);
    let router = make_router(
        svc.clone(),
        opts_create_and_link(children.clone(), require_non_empty_name_validator(), None),
    );
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/1/roles",
        Some(json!({ "name": "engineer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("name").unwrap(), &json!("engineer"));
    let new_child_id = body.get("id").and_then(|v| v.as_i64()).unwrap();
    let stored_children = children.all();
    assert_eq!(stored_children.len(), 1);
    assert_eq!(stored_children[0].get("name").unwrap(), &json!("engineer"));
    let stored_junctions = svc.all();
    assert_eq!(stored_junctions.len(), 1);
    assert_eq!(stored_junctions[0].get("user_id").unwrap(), &json!(1));
    assert_eq!(
        stored_junctions[0].get("role_id").unwrap(),
        &json!(new_child_id)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn create_and_link_stamps_parent_id_into_child_body_when_child_parent_fk_field_is_set() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![]);
    let router = make_router(
        svc,
        opts_create_and_link(
            children.clone(),
            require_non_empty_name_validator(),
            Some("owner_id".to_string()),
        ),
    );
    let (status, _) = send(
        router,
        Method::POST,
        "/api/users/7/roles",
        Some(json!({ "name": "engineer" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let stored = children.all();
    assert_eq!(stored[0].get("owner_id").unwrap(), &json!(7));
    assert_eq!(stored[0].get("name").unwrap(), &json!("engineer"));
}

#[tokio::test(flavor = "current_thread")]
async fn create_and_link_returns_400_when_body_fails_child_create_validator() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![]);
    let router = make_router(
        svc.clone(),
        opts_create_and_link(children.clone(), require_non_empty_name_validator(), None),
    );
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/1/roles",
        Some(json!({ "name": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    assert!(
        children.all().is_empty(),
        "child must not be created when validator rejects"
    );
    assert!(
        svc.all().is_empty(),
        "junction must not be created when validator rejects"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn create_and_link_mode_off_falls_back_to_junction_only_when_child_create_validator_none() {
    let svc = Arc::new(JunctionService::new());
    let children = ChildStore::with_rows(vec![child_row(10, "admin")]);
    let router = make_router(
        svc.clone(),
        opts_with_child_service(children.clone(), LinkVerb::Put),
    );
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/1/roles",
        Some(json!({ "role_id": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("user_id").unwrap(), &json!(1));
    assert_eq!(body.get("role_id").unwrap(), &json!(42));
    assert!(
        body.get("name").is_none(),
        "junction row should not carry child fields"
    );
    assert_eq!(
        children.all().len(),
        1,
        "no new child should be added in junction-only mode"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn create_and_link_mode_off_falls_back_to_junction_only_when_child_service_none() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(
        svc.clone(),
        BuildOpts {
            child_create_validator: Some(require_non_empty_name_validator()),
            ..BuildOpts::default()
        },
    );
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/1/roles",
        Some(json!({ "role_id": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("user_id").unwrap(), &json!(1));
    assert_eq!(body.get("role_id").unwrap(), &json!(42));
    assert_eq!(svc.all().len(), 1);
}

struct NoIdReturningChildService;

#[async_trait]
impl CrudService for NoIdReturningChildService {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        Ok(Vec::new())
    }
    async fn find(&self, _id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        Ok(None)
    }
    async fn add(&self, body: RowMap) -> Result<RowMap, RepositoryError> {
        let mut r = body;
        r.remove("id");
        Ok(r)
    }
    async fn update(
        &self,
        _id: &Value,
        body: RowMap,
        _e: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        Ok(Some(body))
    }
    async fn delete(&self, _id: &Value, _e: Option<&str>) -> Result<bool, RepositoryError> {
        Ok(false)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn create_and_link_returns_500_when_created_child_has_no_id_column() {
    let svc = Arc::new(JunctionService::new());
    let router = make_router(
        svc.clone(),
        BuildOpts {
            child_service: Some(Arc::new(NoIdReturningChildService)),
            child_create_validator: Some(require_non_empty_name_validator()),
            ..BuildOpts::default()
        },
    );
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/1/roles",
        Some(json!({ "name": "engineer" })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("missing 'id'"), "message: {}", msg);
    assert!(
        svc.all().is_empty(),
        "junction must NOT be created when child has no id"
    );
}
