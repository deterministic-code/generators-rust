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
    create_nested_crud_router, CoerceRowFn, CrudService, EnrichItemFn, EnrichItemsFn, IdType,
    NestedCrudRouterConfig, ResolveItemFn, ValidatorFn,
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
    fn last_update_expected(&self) -> Option<String> {
        self.state.lock().unwrap().last_update_expected.clone()
    }
    fn last_delete_expected(&self) -> Option<String> {
        self.state.lock().unwrap().last_delete_expected.clone()
    }
}

fn value_to_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
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

async fn add_row(svc: &MapService, parent_id: i64, extra: &[(&str, Value)]) -> i64 {
    let mut row = RowMap::new();
    row.insert("user_id".to_string(), Value::from(parent_id));
    for (k, v) in extra {
        row.insert((*k).to_string(), v.clone());
    }
    let stored = svc.add(row).await.unwrap();
    stored.get("id").and_then(|v| v.as_i64()).unwrap()
}

struct BuildCfg {
    use_optimistic_concurrency: bool,
    create_validator: Option<ValidatorFn>,
    update_validator: Option<ValidatorFn>,
    patch_validator: Option<ValidatorFn>,
    coerce_row: Option<CoerceRowFn>,
    enrich_items: Option<EnrichItemsFn>,
    enrich_item: Option<EnrichItemFn>,
    resolve_item: Option<ResolveItemFn>,
}

impl BuildCfg {
    fn default() -> Self {
        Self {
            use_optimistic_concurrency: false,
            create_validator: None,
            update_validator: None,
            patch_validator: None,
            coerce_row: None,
            enrich_items: None,
            enrich_item: None,
            resolve_item: None,
        }
    }
}

fn make_router(svc: Arc<MapService>, opts: BuildCfg) -> Router {
    create_nested_crud_router(NestedCrudRouterConfig {
        service: svc,
        entity_name: "Comment".to_string(),
        base_path: "/api/users/{user_id}/comments".to_string(),
        parent_param_name: "user_id".to_string(),
        parent_fk_field: "user_id".to_string(),
        parent_id_type: IdType::Integer,
        child_id_type: IdType::Integer,
        child_primary_key_param: None,
        use_optimistic_concurrency: opts.use_optimistic_concurrency,
        create_validator: opts.create_validator,
        update_validator: opts.update_validator,
        patch_validator: opts.patch_validator,
        coerce_row: opts.coerce_row,
        enrich_items: opts.enrich_items,
        enrich_item: opts.enrich_item,
        resolve_item: opts.resolve_item,
    })
}

fn custom_router() -> Router {
    Router::new().route(
        "/api/custom-thing",
        get(|| async { axum::Json(json!({ "custom": "yes" })) }),
    )
}

async fn send(
    app: Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    for (k, v) in headers {
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
async fn list_scopes_to_the_parent_and_filters_out_other_parents_rows() {
    let svc = Arc::new(MapService::new_integer());
    add_row(&svc, 1, &[("text", Value::String("a".into()))]).await;
    add_row(&svc, 2, &[("text", Value::String("b".into()))]).await;
    add_row(&svc, 1, &[("text", Value::String("c".into()))]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(router, Method::GET, "/api/users/1/comments", None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item.get("user_id").unwrap(), &json!(1));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn list_returns_400_when_parent_id_is_not_a_positive_integer() {
    let svc = Arc::new(MapService::new_integer());
    let router = make_router(svc, BuildCfg::default());
    for path in [
        "/api/users/0/comments",
        "/api/users/-1/comments",
        "/api/users/abc/comments",
    ] {
        let (status, body) = send(router.clone(), Method::GET, path, None, &[]).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "path {}", path);
        assert_error_envelope(&body, "VALIDATION_ERROR");
        let msg = body["errors"][0]["message"].as_str().unwrap();
        assert!(msg.contains("user_id"), "message: {}", msg);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn list_returns_empty_items_when_parent_exists_but_has_no_children() {
    let svc = Arc::new(MapService::new_integer());
    add_row(&svc, 2, &[]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(router, Method::GET, "/api/users/1/comments", None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "items": [] }));
}

#[tokio::test(flavor = "current_thread")]
async fn get_returns_child_when_it_belongs_to_the_parent() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[("text", Value::String("hi".into()))]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(
        router,
        Method::GET,
        &format!("/api/users/1/comments/{}", child_id),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(child_id));
}

#[tokio::test(flavor = "current_thread")]
async fn get_returns_404_when_child_exists_but_belongs_to_a_different_parent() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 2, &[]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(
        router,
        Method::GET,
        &format!("/api/users/1/comments/{}", child_id),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn get_returns_404_when_child_does_not_exist() {
    let svc = Arc::new(MapService::new_integer());
    add_row(&svc, 1, &[]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(router, Method::GET, "/api/users/1/comments/999", None, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn get_returns_400_when_child_id_is_not_positive_integer() {
    let svc = Arc::new(MapService::new_integer());
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(router, Method::GET, "/api/users/1/comments/abc", None, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
}

#[tokio::test(flavor = "current_thread")]
async fn post_auto_pins_parent_fk_and_returns_201() {
    let svc = Arc::new(MapService::new_integer());
    let router = make_router(svc.clone(), BuildCfg::default());
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/7/comments",
        Some(json!({ "text": "hi" })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("user_id").unwrap(), &json!(7));
    assert_eq!(body.get("text").unwrap(), &json!("hi"));
}

#[tokio::test(flavor = "current_thread")]
async fn post_overrides_client_supplied_parent_fk_with_url_parent_id() {
    let svc = Arc::new(MapService::new_integer());
    let router = make_router(svc.clone(), BuildCfg::default());
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/7/comments",
        Some(json!({ "user_id": 99, "text": "hi" })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("user_id").unwrap(), &json!(7));
}

#[tokio::test(flavor = "current_thread")]
async fn post_returns_400_when_parent_id_is_invalid() {
    let svc = Arc::new(MapService::new_integer());
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/-1/comments",
        Some(json!({ "text": "x" })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
}

#[tokio::test(flavor = "current_thread")]
async fn put_updates_child_belonging_to_parent_and_re_pins_fk() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 4, &[("text", Value::String("orig".into()))]).await;
    let router = make_router(svc.clone(), BuildCfg::default());
    let (status, body) = send(
        router,
        Method::PUT,
        &format!("/api/users/4/comments/{}", child_id),
        Some(json!({ "user_id": 999, "text": "put" })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("text").unwrap(), &json!("put"));
    assert_eq!(body.get("user_id").unwrap(), &json!(4));
}

#[tokio::test(flavor = "current_thread")]
async fn put_returns_404_when_child_belongs_to_different_parent() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 2, &[]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(
        router,
        Method::PUT,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "x" })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn patch_updates_child_belonging_to_parent() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 5, &[("text", Value::String("o".into()))]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, body) = send(
        router,
        Method::PATCH,
        &format!("/api/users/5/comments/{}", child_id),
        Some(json!({ "text": "patched" })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("text").unwrap(), &json!("patched"));
}

#[tokio::test(flavor = "current_thread")]
async fn patch_returns_404_when_child_belongs_to_different_parent() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 2, &[]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, _) = send(
        router,
        Method::PATCH,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "x" })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "current_thread")]
async fn delete_removes_child_belonging_to_parent_and_returns_success_true() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 3, &[]).await;
    let router = make_router(svc.clone(), BuildCfg::default());
    let (status, body) = send(
        router,
        Method::DELETE,
        &format!("/api/users/3/comments/{}", child_id),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "success": true }));
    let after = svc.find(&Value::from(child_id)).await.unwrap();
    assert!(after.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn delete_returns_404_when_child_belongs_to_different_parent() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 2, &[]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, _) = send(
        router,
        Method::DELETE,
        &format!("/api/users/1/comments/{}", child_id),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "current_thread")]
async fn delete_returns_404_when_child_does_not_exist() {
    let svc = Arc::new(MapService::new_integer());
    add_row(&svc, 1, &[]).await;
    let router = make_router(svc, BuildCfg::default());
    let (status, _) = send(
        router,
        Method::DELETE,
        "/api/users/1/comments/999",
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

fn require_text_validator() -> ValidatorFn {
    Arc::new(|body: &RowMap| {
        if body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            Err(vec!["text: required".to_string()])
        } else {
            Ok(())
        }
    })
}

#[tokio::test(flavor = "current_thread")]
async fn create_validator_gates_post_with_400_and_validation_error_code() {
    let svc = Arc::new(MapService::new_integer());
    let router = make_router(
        svc,
        BuildCfg {
            create_validator: Some(require_text_validator()),
            ..BuildCfg::default()
        },
    );
    let (status, body) = send(
        router,
        Method::POST,
        "/api/users/1/comments",
        Some(json!({})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
}

#[tokio::test(flavor = "current_thread")]
async fn update_validator_gates_put_and_patch_with_400() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[("text", Value::String("orig".into()))]).await;
    let router = make_router(
        svc,
        BuildCfg {
            update_validator: Some(require_text_validator()),
            ..BuildCfg::default()
        },
    );
    let (put_status, _) = send(
        router.clone(),
        Method::PUT,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({})),
        &[],
    )
    .await;
    assert_eq!(put_status, StatusCode::BAD_REQUEST);
    let (patch_status, _) = send(
        router,
        Method::PATCH,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({})),
        &[],
    )
    .await;
    assert_eq!(patch_status, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "current_thread")]
async fn coerce_row_transforms_rows_on_list_and_get_and_create_and_update() {
    let svc = Arc::new(MapService::new_integer());
    add_row(&svc, 1, &[("flag", Value::from(1i64))]).await;
    let router = make_router(
        svc.clone(),
        BuildCfg {
            coerce_row: Some(Arc::new(|row: &mut RowMap| {
                if let Some(v) = row.get("flag").and_then(|v| v.as_i64()) {
                    row.insert("flag".to_string(), Value::Bool(v != 0));
                }
            })),
            ..BuildCfg::default()
        },
    );
    let (list_status, list_body) = send(
        router.clone(),
        Method::GET,
        "/api/users/1/comments",
        None,
        &[],
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    let items = list_body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items[0].get("flag").unwrap(), &json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_items_runs_on_list() {
    let svc = Arc::new(MapService::new_integer());
    add_row(&svc, 1, &[]).await;
    let hook: EnrichItemsFn = Arc::new(|rows: Vec<RowMap>| {
        Box::pin(async move {
            let out: Vec<RowMap> = rows
                .into_iter()
                .map(|mut r| {
                    r.insert("tag".to_string(), Value::String("list".into()));
                    r
                })
                .collect();
            Ok(out)
        })
    });
    let router = make_router(
        svc,
        BuildCfg {
            enrich_items: Some(hook),
            ..BuildCfg::default()
        },
    );
    let (status, body) = send(router, Method::GET, "/api/users/1/comments", None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items[0].get("tag").unwrap(), &json!("list"));
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_item_runs_on_get_create_put_patch() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[]).await;
    let hook: EnrichItemFn = Arc::new(|mut row: RowMap| {
        Box::pin(async move {
            row.insert("tag".to_string(), Value::String("item".into()));
            Ok(row)
        })
    });
    let router = make_router(
        svc,
        BuildCfg {
            enrich_item: Some(hook),
            ..BuildCfg::default()
        },
    );
    let (get_status, get_body) = send(
        router.clone(),
        Method::GET,
        &format!("/api/users/1/comments/{}", child_id),
        None,
        &[],
    )
    .await;
    assert_eq!(get_status, StatusCode::OK);
    assert_eq!(get_body.get("tag").unwrap(), &json!("item"));
    let (post_status, post_body) = send(
        router.clone(),
        Method::POST,
        "/api/users/1/comments",
        Some(json!({ "text": "x" })),
        &[],
    )
    .await;
    assert_eq!(post_status, StatusCode::CREATED);
    assert_eq!(post_body.get("tag").unwrap(), &json!("item"));
    let (put_status, put_body) = send(
        router.clone(),
        Method::PUT,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "y" })),
        &[],
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    assert_eq!(put_body.get("tag").unwrap(), &json!("item"));
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "z" })),
        &[],
    )
    .await;
    assert_eq!(patch_status, StatusCode::OK);
    assert_eq!(patch_body.get("tag").unwrap(), &json!("item"));
}

#[tokio::test(flavor = "current_thread")]
async fn resolve_item_rewrites_body_before_create_and_update() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[]).await;
    let hook: ResolveItemFn = Arc::new(|mut body: RowMap| {
        Box::pin(async move {
            if let Some(v) = body.get("text").and_then(|v| v.as_str()) {
                body.insert("text".to_string(), Value::String(v.to_lowercase()));
            }
            Ok(body)
        })
    });
    let router = make_router(
        svc.clone(),
        BuildCfg {
            resolve_item: Some(hook),
            ..BuildCfg::default()
        },
    );
    let (post_status, post_body) = send(
        router.clone(),
        Method::POST,
        "/api/users/1/comments",
        Some(json!({ "text": "UPPER" })),
        &[],
    )
    .await;
    assert_eq!(post_status, StatusCode::CREATED);
    assert_eq!(post_body.get("text").unwrap(), &json!("upper"));
    let (put_status, put_body) = send(
        router,
        Method::PUT,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "PUT" })),
        &[],
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    assert_eq!(put_body.get("text").unwrap(), &json!("put"));
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_returns_428_on_missing_if_match_for_put_patch_delete() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[]).await;
    let router = make_router(
        svc,
        BuildCfg {
            use_optimistic_concurrency: true,
            ..BuildCfg::default()
        },
    );
    for method in [Method::PUT, Method::PATCH, Method::DELETE] {
        let (status, body) = send(
            router.clone(),
            method.clone(),
            &format!("/api/users/1/comments/{}", child_id),
            if method == Method::DELETE {
                None
            } else {
                Some(json!({ "text": "x" }))
            },
            &[],
        )
        .await;
        assert_eq!(
            status,
            StatusCode::PRECONDITION_REQUIRED,
            "method {} should require If-Match",
            method,
        );
        assert_error_envelope(&body, "PRECONDITION_REQUIRED");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn optimistic_concurrency_threads_if_match_value_to_service_update_and_delete() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[]).await;
    let router = make_router(
        svc.clone(),
        BuildCfg {
            use_optimistic_concurrency: true,
            ..BuildCfg::default()
        },
    );
    let (put_status, _) = send(
        router.clone(),
        Method::PUT,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "x" })),
        &[("if-match", "token-put")],
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    assert_eq!(svc.last_update_expected().as_deref(), Some("token-put"));
    let (del_status, _) = send(
        router,
        Method::DELETE,
        &format!("/api/users/1/comments/{}", child_id),
        None,
        &[("if-match", "token-del")],
    )
    .await;
    assert_eq!(del_status, StatusCode::OK);
    assert_eq!(svc.last_delete_expected().as_deref(), Some("token-del"));
}

#[tokio::test(flavor = "current_thread")]
async fn custom_route_still_works_when_merged_with_nested_crud_router() {
    let svc = Arc::new(MapService::new_integer());
    let router = Router::new()
        .merge(make_router(svc, BuildCfg::default()))
        .merge(custom_router());
    let (status, body) = send(router, Method::GET, "/api/custom-thing", None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "custom": "yes" }));
}

fn require_tag_validator(tag: &'static str) -> ValidatorFn {
    Arc::new(move |_body: &RowMap| Err(vec![format!("{}: required", tag)]))
}

#[tokio::test(flavor = "current_thread")]
async fn nested_patch_validator_gates_patch_but_not_put() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[("text", Value::String("orig".into()))]).await;
    let router = make_router(
        svc,
        BuildCfg {
            patch_validator: Some(require_tag_validator("patch_only")),
            ..BuildCfg::default()
        },
    );
    let (put_status, _) = send(
        router.clone(),
        Method::PUT,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "put-through" })),
        &[],
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "patched" })),
        &[],
    )
    .await;
    assert_eq!(patch_status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&patch_body, "VALIDATION_ERROR");
    let msg = patch_body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("patch_only: required"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn nested_patch_falls_back_to_update_validator_when_patch_is_none() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[("text", Value::String("orig".into()))]).await;
    let router = make_router(
        svc,
        BuildCfg {
            update_validator: Some(require_text_validator()),
            ..BuildCfg::default()
        },
    );
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({})),
        &[],
    )
    .await;
    assert_eq!(patch_status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&patch_body, "VALIDATION_ERROR");
    let msg = patch_body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("text: required"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn nested_patch_and_update_validators_apply_independently_when_both_set() {
    let svc = Arc::new(MapService::new_integer());
    let child_id = add_row(&svc, 1, &[("text", Value::String("orig".into()))]).await;
    let router = make_router(
        svc,
        BuildCfg {
            update_validator: Some(require_tag_validator("update_only")),
            patch_validator: Some(require_tag_validator("patch_only")),
            ..BuildCfg::default()
        },
    );
    let (put_status, put_body) = send(
        router.clone(),
        Method::PUT,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "x" })),
        &[],
    )
    .await;
    assert_eq!(put_status, StatusCode::BAD_REQUEST);
    let put_msg = put_body["errors"][0]["message"].as_str().unwrap();
    assert!(
        put_msg.contains("update_only: required"),
        "put msg: {}",
        put_msg
    );
    let (patch_status, patch_body) = send(
        router,
        Method::PATCH,
        &format!("/api/users/1/comments/{}", child_id),
        Some(json!({ "text": "x" })),
        &[],
    )
    .await;
    assert_eq!(patch_status, StatusCode::BAD_REQUEST);
    let patch_msg = patch_body["errors"][0]["message"].as_str().unwrap();
    assert!(
        patch_msg.contains("patch_only: required"),
        "patch msg: {}",
        patch_msg
    );
}
