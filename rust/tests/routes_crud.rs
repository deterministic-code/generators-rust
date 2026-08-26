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
    create_crud_router, CoerceRowFn, CrudRouterConfig, CrudService, EnrichItemFn, EnrichItemsFn,
    IdType, ResolveItemFn, ValidatorFn,
};

#[derive(Default)]
struct MapService {
    state: Mutex<MapState>,
    integer_ids: bool,
}

#[derive(Default)]
struct MapState {
    rows: BTreeMap<String, RowMap>,
    next_int: i64,
    last_update_expected: Option<String>,
    last_delete_expected: Option<String>,
}

impl MapService {
    fn new_integer() -> Self {
        Self {
            state: Mutex::new(MapState {
                rows: BTreeMap::new(),
                next_int: 1,
                last_update_expected: None,
                last_delete_expected: None,
            }),
            integer_ids: true,
        }
    }
    fn new_string() -> Self {
        Self {
            state: Mutex::new(MapState::default()),
            integer_ids: false,
        }
    }
    fn last_update_expected(&self) -> Option<String> {
        self.state.lock().unwrap().last_update_expected.clone()
    }
    fn last_delete_expected(&self) -> Option<String> {
        self.state.lock().unwrap().last_delete_expected.clone()
    }
    fn seed(&self, id: Value, row: RowMap) {
        let mut state = self.state.lock().unwrap();
        state.rows.insert(value_to_key(&id), row);
    }
}

fn value_to_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Object(obj) => {
            let mut parts: Vec<_> = obj.iter().collect();
            parts.sort_by(|a, b| a.0.cmp(b.0));
            parts
                .into_iter()
                .map(|(k, val)| format!("{k}={}", value_to_key(val)))
                .collect::<Vec<_>>()
                .join("/")
        }
        other => other.to_string(),
    }
}

#[async_trait]
impl CrudService for MapService {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        let state = self.state.lock().unwrap();
        Ok(state.rows.values().cloned().collect())
    }
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let key = value_to_key(id);
        let state = self.state.lock().unwrap();
        Ok(state.rows.get(&key).cloned())
    }
    async fn add(&self, body: RowMap) -> Result<RowMap, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        let mut row = body;
        if self.integer_ids {
            let id = state.next_int;
            state.next_int += 1;
            row.insert("id".to_string(), Value::from(id));
            state.rows.insert(id.to_string(), row.clone());
        } else {
            let key = row
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("missing-key")
                .to_string();
            state.rows.insert(key, row.clone());
        }
        Ok(row)
    }
    async fn update(
        &self,
        id: &Value,
        body: RowMap,
        expected_updated: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        let key = value_to_key(id);
        let mut state = self.state.lock().unwrap();
        state.last_update_expected = expected_updated.map(|s| s.to_string());
        let Some(row) = state.rows.get_mut(&key) else {
            return Ok(None);
        };
        for (k, v) in body {
            row.insert(k, v);
        }
        Ok(Some(row.clone()))
    }
    async fn delete(
        &self,
        id: &Value,
        expected_updated: Option<&str>,
    ) -> Result<bool, RepositoryError> {
        let key = value_to_key(id);
        let mut state = self.state.lock().unwrap();
        state.last_delete_expected = expected_updated.map(|s| s.to_string());
        Ok(state.rows.remove(&key).is_some())
    }
}

async fn integer_service_seeded(count: usize) -> Arc<MapService> {
    let svc = Arc::new(MapService::new_integer());
    for i in 1..=count {
        let mut row = RowMap::new();
        row.insert("name".to_string(), Value::String(format!("user-{}", i)));
        svc.add(row).await.unwrap();
    }
    svc
}

fn integer_router(svc: Arc<MapService>) -> Router {
    create_crud_router(CrudRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        id_type: IdType::Integer,
        primary_key_param: None,
        primary_key_params: vec![],
        use_optimistic_concurrency: false,
        create_validator: None,
        update_validator: None,
        patch_validator: None,
        coerce_row: None,
        enrich_items: None,
        enrich_item: None,
        resolve_item: None,
    })
}

fn custom_router() -> Router {
    Router::new().route(
        "/api/custom-thing",
        get(|| async { axum::Json(json!({ "custom": "yes" })) }),
    )
}

fn merged(svc: Arc<MapService>) -> Router {
    Router::new()
        .merge(integer_router(svc))
        .merge(custom_router())
}

async fn send(app: Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    send_with_headers(app, method, uri, body, &[]).await
}

async fn send_with_headers(
    app: Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    extra_headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
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
async fn list_returns_empty_items_for_no_rows() {
    let svc = Arc::new(MapService::new_integer());
    let (status, body) = send(merged(svc), Method::GET, "/api/users", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "items": [] }));
}

#[tokio::test(flavor = "current_thread")]
async fn list_returns_seeded_rows() {
    let svc = integer_service_seeded(3).await;
    let (status, body) = send(merged(svc), Method::GET, "/api/users", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn list_returns_ten_seeded_rows() {
    let svc = integer_service_seeded(10).await;
    let (status, body) = send(merged(svc), Method::GET, "/api/users", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.get("items").and_then(|v| v.as_array()).unwrap().len(),
        10,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn get_returns_row_for_valid_id() {
    let svc = integer_service_seeded(2).await;
    let (status, body) = send(merged(svc), Method::GET, "/api/users/1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(1));
    assert_eq!(body.get("name").unwrap(), &json!("user-1"));
}

#[tokio::test(flavor = "current_thread")]
async fn get_returns_404_for_missing_id() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send(merged(svc), Method::GET, "/api/users/999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("User"), "message includes entity: {}", msg);
    assert!(msg.contains("999"), "message includes id: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn get_returns_400_for_invalid_id() {
    for path in ["/api/users/0", "/api/users/-5", "/api/users/abc"] {
        let svc = integer_service_seeded(1).await;
        let (status, body) = send(merged(svc), Method::GET, path, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "path {}", path);
        assert_error_envelope(&body, "VALIDATION_ERROR");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn post_returns_201_with_created_row() {
    let svc = Arc::new(MapService::new_integer());
    let (status, body) = send(
        merged(svc),
        Method::POST,
        "/api/users",
        Some(json!({ "name": "alice" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("id").unwrap(), &json!(1));
    assert_eq!(body.get("name").unwrap(), &json!("alice"));
}

#[tokio::test(flavor = "current_thread")]
async fn put_updates_row_and_returns_200() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send(
        merged(svc),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "renamed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("name").unwrap(), &json!("renamed"));
}

#[tokio::test(flavor = "current_thread")]
async fn put_strips_id_from_body_so_pk_cannot_be_rewritten() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send(
        merged(svc.clone()),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "id": 42, "name": "still-one" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(1));
}

#[tokio::test(flavor = "current_thread")]
async fn put_returns_404_for_missing_id() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send(
        merged(svc),
        Method::PUT,
        "/api/users/999",
        Some(json!({ "name": "ghost" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn patch_updates_row_and_returns_200() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send(
        merged(svc),
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "patched" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("name").unwrap(), &json!("patched"));
}

#[tokio::test(flavor = "current_thread")]
async fn delete_returns_success_true_and_removes_row() {
    let svc = integer_service_seeded(1).await;
    let app = merged(svc.clone());
    let (status, body) = send(app, Method::DELETE, "/api/users/1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "success": true }));
    let after = svc.find(&Value::from(1i64)).await.unwrap();
    assert!(after.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn delete_returns_404_for_missing_id() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send(merged(svc), Method::DELETE, "/api/users/999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn custom_route_still_works_when_merged_with_generated() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send(merged(svc), Method::GET, "/api/custom-thing", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "custom": "yes" }));
}

fn composite_router(svc: Arc<MapService>) -> Router {
    create_crud_router(CrudRouterConfig {
        service: svc,
        entity_name: "Link".to_string(),
        base_path: "/api/links".to_string(),
        id_type: IdType::Integer,
        primary_key_param: None,
        primary_key_params: vec!["left_id".to_string(), "right_id".to_string()],
        use_optimistic_concurrency: false,
        create_validator: None,
        update_validator: None,
        patch_validator: None,
        coerce_row: None,
        enrich_items: None,
        enrich_item: None,
        resolve_item: None,
    })
}

fn seeded_link() -> (Arc<MapService>, RowMap) {
    let svc = Arc::new(MapService::new_integer());
    let mut row = RowMap::new();
    row.insert("left_id".to_string(), Value::from(1i64));
    row.insert("right_id".to_string(), Value::from(2i64));
    row.insert("label".to_string(), Value::from("ab"));
    svc.seed(json!({ "left_id": 1, "right_id": 2 }), row.clone());
    (svc, row)
}

#[tokio::test(flavor = "current_thread")]
async fn composite_get_passes_both_keys() {
    let (svc, row) = seeded_link();
    let (status, body) = send(composite_router(svc), Method::GET, "/api/links/1/2", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["left_id"], json!(1));
    assert_eq!(body["right_id"], json!(2));
    assert_eq!(body["label"], json!("ab"));
    let _ = row;
}

#[tokio::test(flavor = "current_thread")]
async fn composite_patch_updates_by_both_keys() {
    let (svc, _) = seeded_link();
    let (status, body) = send(
        composite_router(svc),
        Method::PATCH,
        "/api/links/1/2",
        Some(json!({ "label": "changed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["label"], json!("changed"));
}

#[tokio::test(flavor = "current_thread")]
async fn composite_delete_removes_by_both_keys() {
    let (svc, _) = seeded_link();
    let (status, body) = send(composite_router(svc), Method::DELETE, "/api/links/1/2", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], json!(true));
}

fn string_pk_router(svc: Arc<MapService>) -> Router {
    create_crud_router(CrudRouterConfig {
        service: svc,
        entity_name: "Mailbox".to_string(),
        base_path: "/api/mailboxes".to_string(),
        id_type: IdType::String,
        primary_key_param: Some("key".to_string()),
        primary_key_params: vec![],
        use_optimistic_concurrency: false,
        create_validator: None,
        update_validator: None,
        patch_validator: None,
        coerce_row: None,
        enrich_items: None,
        enrich_item: None,
        resolve_item: None,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn string_pk_get_returns_row_at_custom_path_segment() {
    let svc = Arc::new(MapService::new_string());
    svc.add({
        let mut r = RowMap::new();
        r.insert("key".to_string(), Value::String("alpha".to_string()));
        r.insert("label".to_string(), Value::String("Alpha".to_string()));
        r
    })
    .await
    .unwrap();
    let router = string_pk_router(svc);
    let (status, body) = send(router, Method::GET, "/api/mailboxes/alpha", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("key").unwrap(), &json!("alpha"));
    assert_eq!(body.get("label").unwrap(), &json!("Alpha"));
}

#[tokio::test(flavor = "current_thread")]
async fn string_pk_get_returns_400_for_empty_id_segment() {
    let svc = Arc::new(MapService::new_string());
    let router = string_pk_router(svc);
    let (status, body) = send(router, Method::GET, "/api/mailboxes/%20", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
}

fn uuid_router(svc: Arc<MapService>) -> Router {
    create_crud_router(CrudRouterConfig {
        service: svc,
        entity_name: "Session".to_string(),
        base_path: "/api/sessions".to_string(),
        id_type: IdType::Uuid,
        primary_key_param: None,
        primary_key_params: vec![],
        use_optimistic_concurrency: false,
        create_validator: None,
        update_validator: None,
        patch_validator: None,
        coerce_row: None,
        enrich_items: None,
        enrich_item: None,
        resolve_item: None,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn uuid_pk_get_returns_400_for_non_uuid() {
    let svc = Arc::new(MapService::new_string());
    let router = uuid_router(svc);
    let (status, body) = send(router, Method::GET, "/api/sessions/not-a-uuid", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("uuid"), "message hints uuid: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn uuid_pk_get_dispatches_to_service_for_valid_uuid() {
    let svc = Arc::new(MapService::new_string());
    let uuid_val = "550e8400-e29b-41d4-a716-446655440000";
    let router = uuid_router(svc);
    let (status, body) = send(
        router,
        Method::GET,
        &format!("/api/sessions/{}", uuid_val),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains(uuid_val), "message contains uuid: {}", msg);
}

fn optimistic_router(svc: Arc<MapService>) -> Router {
    create_crud_router(CrudRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        id_type: IdType::Integer,
        primary_key_param: None,
        primary_key_params: vec![],
        use_optimistic_concurrency: true,
        create_validator: None,
        update_validator: None,
        patch_validator: None,
        coerce_row: None,
        enrich_items: None,
        enrich_item: None,
        resolve_item: None,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_returns_428_when_if_match_missing_on_put() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send(
        optimistic_router(svc),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    assert_error_envelope(&body, "PRECONDITION_REQUIRED");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("PUT"), "message mentions method: {}", msg);
    assert!(msg.contains("User"), "message mentions entity: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_returns_428_on_patch_and_delete_when_missing() {
    let svc = integer_service_seeded(1).await;
    let app = optimistic_router(svc);
    let (patch_status, _) = send(
        app.clone(),
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(patch_status, StatusCode::PRECONDITION_REQUIRED);
    let (delete_status, _) = send(app, Method::DELETE, "/api/users/1", None).await;
    assert_eq!(delete_status, StatusCode::PRECONDITION_REQUIRED);
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_lets_mutation_through_when_if_match_present() {
    let svc = integer_service_seeded(1).await;
    let (status, body) = send_with_headers(
        optimistic_router(svc),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "yay" })),
        &[("if-match", "any-token")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("name").unwrap(), &json!("yay"));
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_gate_ignored_when_flag_false() {
    let svc = integer_service_seeded(1).await;
    let (status, _) = send(
        merged(svc),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "no-gate" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_threads_if_match_value_to_service_update_on_put() {
    let svc = integer_service_seeded(1).await;
    let (status, _) = send_with_headers(
        optimistic_router(svc.clone()),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "put" })),
        &[("if-match", "token-put")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(svc.last_update_expected().as_deref(), Some("token-put"));
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_threads_if_match_value_to_service_update_on_patch() {
    let svc = integer_service_seeded(1).await;
    let (status, _) = send_with_headers(
        optimistic_router(svc.clone()),
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "patch" })),
        &[("if-match", "token-patch")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(svc.last_update_expected().as_deref(), Some("token-patch"));
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_threads_if_match_value_to_service_delete() {
    let svc = integer_service_seeded(1).await;
    let (status, _) = send_with_headers(
        optimistic_router(svc.clone()),
        Method::DELETE,
        "/api/users/1",
        None,
        &[("if-match", "token-delete")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(svc.last_delete_expected().as_deref(), Some("token-delete"));
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_strips_surrounding_quotes_from_etag_style_if_match() {
    let svc = integer_service_seeded(1).await;
    let (status, _) = send_with_headers(
        optimistic_router(svc.clone()),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "quoted" })),
        &[("if-match", "\"quoted-token\"")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(svc.last_update_expected().as_deref(), Some("quoted-token"));
}

#[tokio::test(flavor = "current_thread")]
async fn expected_updated_is_none_when_optimistic_concurrency_is_off() {
    let svc = integer_service_seeded(1).await;
    let (status, _) = send(
        merged(svc.clone()),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "no-gate" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(svc.last_update_expected(), None);
}

fn require_name_validator() -> ValidatorFn {
    Arc::new(|body: &RowMap| {
        let mut errs = Vec::new();
        if body
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            errs.push("name: required".to_string());
        }
        if let Some(age) = body.get("age").and_then(|v| v.as_i64()) {
            if age < 0 {
                errs.push("age: must be non-negative".to_string());
            }
        }
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    })
}

fn hooked_router(
    svc: Arc<MapService>,
    create: Option<ValidatorFn>,
    update: Option<ValidatorFn>,
    coerce: Option<CoerceRowFn>,
) -> Router {
    hooked_router_with_patch(svc, create, update, None, coerce)
}

fn hooked_router_with_patch(
    svc: Arc<MapService>,
    create: Option<ValidatorFn>,
    update: Option<ValidatorFn>,
    patch: Option<ValidatorFn>,
    coerce: Option<CoerceRowFn>,
) -> Router {
    create_crud_router(CrudRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        id_type: IdType::Integer,
        primary_key_param: None,
        primary_key_params: vec![],
        use_optimistic_concurrency: false,
        create_validator: create,
        update_validator: update,
        patch_validator: patch,
        coerce_row: coerce,
        enrich_items: None,
        enrich_item: None,
        resolve_item: None,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn create_validator_rejects_missing_name_with_400() {
    let svc = Arc::new(MapService::new_integer());
    let router = hooked_router(svc, Some(require_name_validator()), None, None);
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users",
        Some(json!({ "not-name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("name: required"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn create_validator_joins_multiple_errors_with_semicolon() {
    let svc = Arc::new(MapService::new_integer());
    let router = hooked_router(svc, Some(require_name_validator()), None, None);
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users",
        Some(json!({ "age": -5 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("name: required"), "message: {}", msg);
    assert!(
        msg.contains("age: must be non-negative"),
        "message: {}",
        msg
    );
    assert!(msg.contains("; "), "compound joined with '; ': {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn create_validator_passes_valid_body_through_to_service() {
    let svc = Arc::new(MapService::new_integer());
    let router = hooked_router(svc, Some(require_name_validator()), None, None);
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users",
        Some(json!({ "name": "alice" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("name").unwrap(), &json!("alice"));
}

#[tokio::test(flavor = "current_thread")]
async fn update_validator_gates_put_and_patch() {
    let svc = integer_service_seeded(1).await;
    let router = hooked_router(svc.clone(), None, Some(require_name_validator()), None);
    let (put_status, put_body) = send(
        router.clone(),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "" })),
    )
    .await;
    assert_eq!(put_status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&put_body, "VALIDATION_ERROR");
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "" })),
    )
    .await;
    assert_eq!(patch_status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&patch_body, "VALIDATION_ERROR");
}

#[tokio::test(flavor = "current_thread")]
async fn coerce_row_transforms_row_before_serialization_on_list_get_and_create() {
    let coerce: CoerceRowFn = Arc::new(|row: &mut RowMap| {
        if let Some(v) = row.get("flag").and_then(|v| v.as_i64()) {
            row.insert("flag".to_string(), Value::Bool(v != 0));
        }
    });
    let svc = Arc::new(MapService::new_integer());
    let router = hooked_router(svc.clone(), None, None, Some(coerce.clone()));
    let (create_status, create_body) = send(
        router.clone(),
        Method::POST,
        "/api/users",
        Some(json!({ "name": "x", "flag": 1 })),
    )
    .await;
    assert_eq!(create_status, StatusCode::CREATED);
    assert_eq!(create_body.get("flag").unwrap(), &json!(true));
    let (get_status, get_body) = send(router.clone(), Method::GET, "/api/users/1", None).await;
    assert_eq!(get_status, StatusCode::OK);
    assert_eq!(get_body.get("flag").unwrap(), &json!(true));
    let (list_status, list_body) = send(router, Method::GET, "/api/users", None).await;
    assert_eq!(list_status, StatusCode::OK);
    let items = list_body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items[0].get("flag").unwrap(), &json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn coerce_row_runs_on_update_and_patch_responses() {
    let coerce: CoerceRowFn = Arc::new(|row: &mut RowMap| {
        if let Some(v) = row.get("flag").and_then(|v| v.as_i64()) {
            row.insert("flag".to_string(), Value::Bool(v != 0));
        }
    });
    let svc = Arc::new(MapService::new_integer());
    svc.add({
        let mut r = RowMap::new();
        r.insert("name".to_string(), Value::String("orig".to_string()));
        r.insert("flag".to_string(), Value::from(1i64));
        r
    })
    .await
    .unwrap();
    let router = hooked_router(svc.clone(), None, None, Some(coerce));
    let (put_status, put_body) = send(
        router.clone(),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "put" })),
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    assert_eq!(put_body.get("flag").unwrap(), &json!(true));
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "patched" })),
    )
    .await;
    assert_eq!(patch_status, StatusCode::OK);
    assert_eq!(patch_body.get("flag").unwrap(), &json!(true));
}

fn enriched_router(
    svc: Arc<MapService>,
    enrich_items: Option<EnrichItemsFn>,
    enrich_item: Option<EnrichItemFn>,
    resolve_item: Option<ResolveItemFn>,
) -> Router {
    create_crud_router(CrudRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        id_type: IdType::Integer,
        primary_key_param: None,
        primary_key_params: vec![],
        use_optimistic_concurrency: false,
        create_validator: None,
        update_validator: None,
        patch_validator: None,
        coerce_row: None,
        enrich_items,
        enrich_item,
        resolve_item,
    })
}

fn tag_items_with_source() -> EnrichItemsFn {
    Arc::new(|rows: Vec<RowMap>| {
        Box::pin(async move {
            let out: Vec<RowMap> = rows
                .into_iter()
                .map(|mut r| {
                    r.insert(
                        "source".to_string(),
                        Value::String("enriched-list".to_string()),
                    );
                    r
                })
                .collect();
            Ok(out)
        })
    })
}

fn tag_item_with_source() -> EnrichItemFn {
    Arc::new(|mut row: RowMap| {
        Box::pin(async move {
            row.insert(
                "source".to_string(),
                Value::String("enriched-item".to_string()),
            );
            Ok(row)
        })
    })
}

fn resolve_normalizes_name_to_lowercase() -> ResolveItemFn {
    Arc::new(|mut body: RowMap| {
        Box::pin(async move {
            if let Some(v) = body.get("name").and_then(|v| v.as_str()) {
                body.insert("name".to_string(), Value::String(v.to_lowercase()));
            }
            Ok(body)
        })
    })
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_items_transforms_every_row_on_list() {
    let svc = integer_service_seeded(3).await;
    let router = enriched_router(svc, Some(tag_items_with_source()), None, None);
    let (status, body) = send(router, Method::GET, "/api/users", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 3);
    for item in items {
        assert_eq!(item.get("source").unwrap(), &json!("enriched-list"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_item_transforms_row_on_get_by_id() {
    let svc = integer_service_seeded(1).await;
    let router = enriched_router(svc, None, Some(tag_item_with_source()), None);
    let (status, body) = send(router, Method::GET, "/api/users/1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("source").unwrap(), &json!("enriched-item"));
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_item_runs_on_create_response() {
    let svc = Arc::new(MapService::new_integer());
    let router = enriched_router(svc, None, Some(tag_item_with_source()), None);
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users",
        Some(json!({ "name": "alice" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("source").unwrap(), &json!("enriched-item"));
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_item_runs_on_update_and_patch_responses() {
    let svc = integer_service_seeded(1).await;
    let router = enriched_router(svc, None, Some(tag_item_with_source()), None);
    let (put_status, put_body) = send(
        router.clone(),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "u" })),
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    assert_eq!(put_body.get("source").unwrap(), &json!("enriched-item"));
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "p" })),
    )
    .await;
    assert_eq!(patch_status, StatusCode::OK);
    assert_eq!(patch_body.get("source").unwrap(), &json!("enriched-item"));
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_item_rewrites_body_before_service_add() {
    let svc = Arc::new(MapService::new_integer());
    let router = enriched_router(
        svc.clone(),
        None,
        None,
        Some(resolve_normalizes_name_to_lowercase()),
    );
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users",
        Some(json!({ "name": "ALICE" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("name").unwrap(), &json!("alice"));
    let stored = svc.find(&Value::from(1i64)).await.unwrap().unwrap();
    assert_eq!(stored.get("name").unwrap(), &json!("alice"));
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_item_rewrites_body_before_service_update_on_put_and_patch() {
    let svc = integer_service_seeded(1).await;
    let router = enriched_router(
        svc.clone(),
        None,
        None,
        Some(resolve_normalizes_name_to_lowercase()),
    );
    let (put_status, put_body) = send(
        router.clone(),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "BOB" })),
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    assert_eq!(put_body.get("name").unwrap(), &json!("bob"));
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "CAROL" })),
    )
    .await;
    assert_eq!(patch_status, StatusCode::OK);
    assert_eq!(patch_body.get("name").unwrap(), &json!("carol"));
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_items_error_returns_500_and_preserves_envelope_shape() {
    let svc = integer_service_seeded(1).await;
    let failing_enrich: EnrichItemsFn = Arc::new(|_rows| {
        Box::pin(async move {
            Err(deterministic::routes::RouteHookError::Repository(
                deterministic::error::RepositoryError::UnsupportedQuery(
                    "enrichment blew up".to_string(),
                ),
            ))
        })
    });
    let router = enriched_router(svc, Some(failing_enrich), None, None);
    let (status, body) = send(router, Method::GET, "/api/users", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let errors = body.get("errors").and_then(|v| v.as_array()).unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].get("code").unwrap(), &json!("INTERNAL_ERROR"));
    let msg = errors[0].get("message").unwrap().as_str().unwrap();
    assert!(msg.contains("enrichment blew up"), "message: {}", msg);
}

fn tag_only_validator(tag: &'static str) -> ValidatorFn {
    Arc::new(move |_body: &RowMap| Err(vec![format!("{}: required", tag)]))
}

#[tokio::test(flavor = "current_thread")]
async fn patch_validator_gates_patch_but_not_put() {
    let svc = integer_service_seeded(1).await;
    let router = hooked_router_with_patch(
        svc,
        None,
        None,
        Some(tag_only_validator("patch_only")),
        None,
    );
    let (put_status, _) = send(
        router.clone(),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "put-through" })),
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "patched" })),
    )
    .await;
    assert_eq!(patch_status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&patch_body, "VALIDATION_ERROR");
    let msg = patch_body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("patch_only: required"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn patch_falls_back_to_update_validator_when_patch_validator_is_none() {
    let svc = integer_service_seeded(1).await;
    let router = hooked_router_with_patch(svc, None, Some(require_name_validator()), None, None);
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "" })),
    )
    .await;
    assert_eq!(patch_status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&patch_body, "VALIDATION_ERROR");
    let msg = patch_body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("name: required"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn patch_and_update_validators_apply_independently_when_both_set() {
    let svc = integer_service_seeded(1).await;
    let router = hooked_router_with_patch(
        svc,
        None,
        Some(tag_only_validator("update_only")),
        Some(tag_only_validator("patch_only")),
        None,
    );
    let (put_status, put_body) = send(
        router.clone(),
        Method::PUT,
        "/api/users/1",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(put_status, StatusCode::BAD_REQUEST);
    let put_msg = put_body["errors"][0]["message"].as_str().unwrap();
    assert!(
        put_msg.contains("update_only: required"),
        "put msg: {}",
        put_msg
    );
    assert!(
        !put_msg.contains("patch_only"),
        "put msg leaks patch tag: {}",
        put_msg
    );
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        "/api/users/1",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(patch_status, StatusCode::BAD_REQUEST);
    let patch_msg = patch_body["errors"][0]["message"].as_str().unwrap();
    assert!(
        patch_msg.contains("patch_only: required"),
        "patch msg: {}",
        patch_msg
    );
    assert!(
        !patch_msg.contains("update_only"),
        "patch msg leaks update tag: {}",
        patch_msg
    );
}
