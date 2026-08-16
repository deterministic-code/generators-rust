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
    create_by_field_router, ByFieldRouterConfig, ByFieldService, CoerceRowFn,
};

#[derive(Default)]
struct MapByFieldService {
    rows: Mutex<Vec<RowMap>>,
    calls: Mutex<Vec<(String, Value)>>,
}

impl MapByFieldService {
    fn new() -> Self {
        Self::default()
    }
    fn seed(&self, rows: Vec<RowMap>) {
        *self.rows.lock().unwrap() = rows;
    }
    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
    fn last_call(&self) -> Option<(String, Value)> {
        self.calls.lock().unwrap().last().cloned()
    }
}

#[async_trait]
impl ByFieldService for MapByFieldService {
    async fn find_by(&self, field: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        self.calls
            .lock()
            .unwrap()
            .push((field.to_string(), value.clone()));
        let rows = self.rows.lock().unwrap();
        Ok(rows
            .iter()
            .filter(|r| r.get(field).map(|v| v == value).unwrap_or(false))
            .cloned()
            .collect())
    }
}

fn row(pairs: &[(&str, Value)]) -> RowMap {
    let mut r: BTreeMap<String, Value> = BTreeMap::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), v.clone());
    }
    r
}

fn make_router(
    svc: Arc<MapByFieldService>,
    field: &str,
    unique: bool,
    coerce_row: Option<CoerceRowFn>,
) -> Router {
    create_by_field_router(ByFieldRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        field: field.to_string(),
        unique,
        methods: vec![deterministic::routes::ByFieldMethod::Get],
        coerce_row,
        update_validator: None,
    })
}

fn custom_router() -> Router {
    Router::new().route(
        "/api/custom-thing",
        get(|| async { axum::Json(json!({ "custom": "yes" })) }),
    )
}

async fn json_get(app: Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
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
async fn non_unique_returns_empty_items_when_no_rows_match() {
    let svc = Arc::new(MapByFieldService::new());
    let router = make_router(svc, "email", false, None);
    let (status, body) = json_get(router, "/api/users/email/nobody@x").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "items": [] }));
}

#[tokio::test(flavor = "current_thread")]
async fn non_unique_returns_single_item_wrapped_in_items_array() {
    let svc = Arc::new(MapByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("email", Value::String("a@x".to_string())),
    ])]);
    let router = make_router(svc, "email", false, None);
    let (status, body) = json_get(router, "/api/users/email/a@x").await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("id").unwrap(), &json!(1));
}

#[tokio::test(flavor = "current_thread")]
async fn non_unique_returns_all_matching_rows() {
    let svc = Arc::new(MapByFieldService::new());
    svc.seed(vec![
        row(&[
            ("id", Value::from(1i64)),
            ("phone", Value::String("555".to_string())),
        ]),
        row(&[
            ("id", Value::from(2i64)),
            ("phone", Value::String("555".to_string())),
        ]),
        row(&[
            ("id", Value::from(3i64)),
            ("phone", Value::String("555".to_string())),
        ]),
    ]);
    let router = make_router(svc, "phone", false, None);
    let (status, body) = json_get(router, "/api/users/phone/555").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.get("items").and_then(|v| v.as_array()).unwrap().len(),
        3,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unique_returns_the_single_matching_row_as_raw_json() {
    let svc = Arc::new(MapByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(7i64)),
        ("email", Value::String("solo@x".to_string())),
    ])]);
    let router = make_router(svc, "email", true, None);
    let (status, body) = json_get(router, "/api/users/email/solo@x").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(7));
    assert!(body.get("items").is_none(), "should be raw row not wrapped");
}

#[tokio::test(flavor = "current_thread")]
async fn unique_returns_404_when_no_rows_match() {
    let svc = Arc::new(MapByFieldService::new());
    let router = make_router(svc, "email", true, None);
    let (status, body) = json_get(router, "/api/users/email/nobody@x").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("User"), "message: {}", msg);
    assert!(msg.contains("email"), "message: {}", msg);
    assert!(msg.contains("nobody@x"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn unique_returns_409_when_multiple_rows_match() {
    let svc = Arc::new(MapByFieldService::new());
    svc.seed(vec![
        row(&[
            ("id", Value::from(1i64)),
            ("email", Value::String("dup@x".to_string())),
        ]),
        row(&[
            ("id", Value::from(2i64)),
            ("email", Value::String("dup@x".to_string())),
        ]),
    ]);
    let router = make_router(svc, "email", true, None);
    let (status, body) = json_get(router, "/api/users/email/dup@x").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "CONFLICT");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("Multiple"), "message: {}", msg);
    assert!(msg.contains("email"), "message: {}", msg);
    assert!(msg.contains("dup@x"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn coerce_row_transforms_rows_before_serialization_in_unique_mode() {
    let coerce: CoerceRowFn = Arc::new(|row: &mut RowMap| {
        if let Some(v) = row.get("flag").and_then(|v| v.as_i64()) {
            row.insert("flag".to_string(), Value::Bool(v != 0));
        }
    });
    let svc = Arc::new(MapByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("email", Value::String("x".to_string())),
        ("flag", Value::from(1i64)),
    ])]);
    let router = make_router(svc, "email", true, Some(coerce));
    let (status, body) = json_get(router, "/api/users/email/x").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("flag").unwrap(), &json!(true));
}

#[tokio::test(flavor = "current_thread")]
async fn coerce_row_transforms_every_row_before_serialization_in_non_unique_mode() {
    let coerce: CoerceRowFn = Arc::new(|row: &mut RowMap| {
        if let Some(v) = row.get("flag").and_then(|v| v.as_i64()) {
            row.insert("flag".to_string(), Value::Bool(v != 0));
        }
    });
    let svc = Arc::new(MapByFieldService::new());
    svc.seed(vec![
        row(&[
            ("email", Value::String("x".to_string())),
            ("flag", Value::from(1i64)),
        ]),
        row(&[
            ("email", Value::String("x".to_string())),
            ("flag", Value::from(0i64)),
        ]),
    ]);
    let router = make_router(svc, "email", false, Some(coerce));
    let (status, body) = json_get(router, "/api/users/email/x").await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].get("flag").unwrap(), &json!(true));
    assert_eq!(items[1].get("flag").unwrap(), &json!(false));
}

#[tokio::test(flavor = "current_thread")]
async fn snake_case_field_gets_kebab_path_segment() {
    let svc = Arc::new(MapByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("profile_pic", Value::String("avatar.png".to_string())),
    ])]);
    let router = make_router(svc, "profile_pic", true, None);
    let (status, body) = json_get(router, "/api/users/profile-pic/avatar.png").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(1));
}

#[tokio::test(flavor = "current_thread")]
async fn service_receives_the_field_name_and_value_from_the_url() {
    let svc = Arc::new(MapByFieldService::new());
    let make_svc = svc.clone();
    let router = make_router(make_svc, "email", false, None);
    let (status, _) = json_get(router, "/api/users/email/a@b.c").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(svc.call_count(), 1);
    let (field, value) = svc.last_call().unwrap();
    assert_eq!(field, "email");
    assert_eq!(value, Value::String("a@b.c".to_string()));
}

#[tokio::test(flavor = "current_thread")]
async fn custom_route_still_responds_when_merged_with_by_field_router() {
    let svc = Arc::new(MapByFieldService::new());
    let router = Router::new()
        .merge(make_router(svc, "email", false, None))
        .merge(custom_router());
    let (status, body) = json_get(router, "/api/custom-thing").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "custom": "yes" }));
}

#[tokio::test(flavor = "current_thread")]
async fn base_path_with_trailing_slash_is_normalized() {
    let svc = Arc::new(MapByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("email", Value::String("norm@x".to_string())),
    ])]);
    let router = create_by_field_router(ByFieldRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        field: "email".to_string(),
        unique: true,
        methods: vec![deterministic::routes::ByFieldMethod::Get],
        coerce_row: None,
        update_validator: None,
    });
    let (status, body) = json_get(router, "/api/users/email/norm@x").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(1));
}

#[derive(Default)]
struct MutableByFieldService {
    rows: Mutex<Vec<RowMap>>,
    update_calls: Mutex<Vec<(String, Value, RowMap)>>,
    delete_calls: Mutex<Vec<(String, Value)>>,
    update_count_override: Mutex<Option<u64>>,
    delete_count_override: Mutex<Option<u64>>,
}

impl MutableByFieldService {
    fn new() -> Self {
        Self::default()
    }
    fn seed(&self, rows: Vec<RowMap>) {
        *self.rows.lock().unwrap() = rows;
    }
    fn all_rows(&self) -> Vec<RowMap> {
        self.rows.lock().unwrap().clone()
    }
    fn set_update_count(&self, count: u64) {
        *self.update_count_override.lock().unwrap() = Some(count);
    }
    fn set_delete_count(&self, count: u64) {
        *self.delete_count_override.lock().unwrap() = Some(count);
    }
    fn last_update_call(&self) -> Option<(String, Value, RowMap)> {
        self.update_calls.lock().unwrap().last().cloned()
    }
    fn last_delete_call(&self) -> Option<(String, Value)> {
        self.delete_calls.lock().unwrap().last().cloned()
    }
}

#[async_trait]
impl ByFieldService for MutableByFieldService {
    async fn find_by(&self, field: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        let rows = self.rows.lock().unwrap();
        Ok(rows
            .iter()
            .filter(|r| r.get(field).map(|v| v == value).unwrap_or(false))
            .cloned()
            .collect())
    }

    async fn update_by(
        &self,
        field: &str,
        value: &Value,
        data: RowMap,
    ) -> Result<u64, RepositoryError> {
        self.update_calls
            .lock()
            .unwrap()
            .push((field.to_string(), value.clone(), data.clone()));
        let override_ = *self.update_count_override.lock().unwrap();
        if let Some(count) = override_ {
            let mut rows = self.rows.lock().unwrap();
            let mut applied = 0u64;
            for row in rows.iter_mut() {
                if row.get(field).map(|v| v == value).unwrap_or(false) {
                    for (k, v) in data.clone() {
                        row.insert(k, v);
                    }
                    applied += 1;
                }
            }
            let _ = applied;
            return Ok(count);
        }
        let mut rows = self.rows.lock().unwrap();
        let mut count = 0u64;
        for row in rows.iter_mut() {
            if row.get(field).map(|v| v == value).unwrap_or(false) {
                for (k, v) in data.clone() {
                    row.insert(k, v);
                }
                count += 1;
            }
        }
        Ok(count)
    }

    async fn delete_by(&self, field: &str, value: &Value) -> Result<u64, RepositoryError> {
        self.delete_calls
            .lock()
            .unwrap()
            .push((field.to_string(), value.clone()));
        let override_ = *self.delete_count_override.lock().unwrap();
        if let Some(count) = override_ {
            let mut rows = self.rows.lock().unwrap();
            rows.retain(|r| r.get(field).map(|v| v != value).unwrap_or(true));
            return Ok(count);
        }
        let mut rows = self.rows.lock().unwrap();
        let before = rows.len();
        rows.retain(|r| r.get(field).map(|v| v != value).unwrap_or(true));
        Ok((before - rows.len()) as u64)
    }
}

fn make_mutable_router(
    svc: Arc<MutableByFieldService>,
    field: &str,
    unique: bool,
    methods: Vec<deterministic::routes::ByFieldMethod>,
    update_validator: Option<deterministic::routes::ValidatorFn>,
) -> Router {
    create_by_field_router(ByFieldRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        field: field.to_string(),
        unique,
        methods,
        coerce_row: None,
        update_validator,
    })
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

#[tokio::test(flavor = "current_thread")]
async fn put_unique_returns_the_updated_row_when_exactly_one_row_matches() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("email", Value::String("a@x".to_string())),
        ("name", Value::String("orig".to_string())),
    ])]);
    let router = make_mutable_router(
        svc.clone(),
        "email",
        true,
        vec![deterministic::routes::ByFieldMethod::Put],
        None,
    );
    let (status, body) = send(
        router,
        Method::PUT,
        "/api/users/email/a@x",
        Some(json!({ "name": "updated" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("name").unwrap(), &json!("updated"));
    assert_eq!(body.get("id").unwrap(), &json!(1));
    let stored = svc.all_rows();
    assert_eq!(stored[0].get("name").unwrap(), &json!("updated"));
}

#[tokio::test(flavor = "current_thread")]
async fn put_unique_returns_404_when_no_rows_match() {
    let svc = Arc::new(MutableByFieldService::new());
    let router = make_mutable_router(
        svc,
        "email",
        true,
        vec![deterministic::routes::ByFieldMethod::Put],
        None,
    );
    let (status, body) = send(
        router,
        Method::PUT,
        "/api/users/email/missing@x",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn put_unique_returns_409_when_multiple_rows_match() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![
        row(&[
            ("id", Value::from(1i64)),
            ("email", Value::String("dup@x".to_string())),
        ]),
        row(&[
            ("id", Value::from(2i64)),
            ("email", Value::String("dup@x".to_string())),
        ]),
    ]);
    let router = make_mutable_router(
        svc,
        "email",
        true,
        vec![deterministic::routes::ByFieldMethod::Put],
        None,
    );
    let (status, body) = send(
        router,
        Method::PUT,
        "/api/users/email/dup@x",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "CONFLICT");
}

#[tokio::test(flavor = "current_thread")]
async fn put_non_unique_returns_200_with_count_of_updated_rows() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![
        row(&[
            ("id", Value::from(1i64)),
            ("phone", Value::String("555".to_string())),
        ]),
        row(&[
            ("id", Value::from(2i64)),
            ("phone", Value::String("555".to_string())),
        ]),
        row(&[
            ("id", Value::from(3i64)),
            ("phone", Value::String("555".to_string())),
        ]),
    ]);
    let router = make_mutable_router(
        svc,
        "phone",
        false,
        vec![deterministic::routes::ByFieldMethod::Put],
        None,
    );
    let (status, body) = send(
        router,
        Method::PUT,
        "/api/users/phone/555",
        Some(json!({ "verified": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "count": 3 }));
}

#[tokio::test(flavor = "current_thread")]
async fn put_service_receives_field_value_and_body_from_the_url_and_request() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("email", Value::String("a@x".to_string())),
    ])]);
    let router = make_mutable_router(
        svc.clone(),
        "email",
        false,
        vec![deterministic::routes::ByFieldMethod::Put],
        None,
    );
    let (status, _) = send(
        router,
        Method::PUT,
        "/api/users/email/a@x",
        Some(json!({ "name": "x", "n": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (field, value, body) = svc.last_update_call().unwrap();
    assert_eq!(field, "email");
    assert_eq!(value, Value::String("a@x".to_string()));
    assert_eq!(body.get("name").unwrap(), &json!("x"));
    assert_eq!(body.get("n").unwrap(), &json!(42));
}

#[tokio::test(flavor = "current_thread")]
async fn put_update_validator_gates_body_with_400() {
    let hook: deterministic::routes::ValidatorFn = Arc::new(|body: &RowMap| {
        if body.get("name").is_some() {
            Ok(())
        } else {
            Err(vec!["name: required".to_string()])
        }
    });
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![row(&[("email", Value::String("a@x".to_string()))])]);
    let router = make_mutable_router(
        svc,
        "email",
        false,
        vec![deterministic::routes::ByFieldMethod::Put],
        Some(hook),
    );
    let (status, body) = send(router, Method::PUT, "/api/users/email/a@x", Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("name: required"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn put_returns_500_when_default_update_by_impl_is_used() {
    let svc: Arc<MapByFieldService> = Arc::new(MapByFieldService::new());
    let router = create_by_field_router(ByFieldRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        field: "email".to_string(),
        unique: false,
        methods: vec![deterministic::routes::ByFieldMethod::Put],
        coerce_row: None,
        update_validator: None,
    });
    let (status, body) = send(
        router,
        Method::PUT,
        "/api/users/email/a@x",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_error_envelope(&body, "INTERNAL_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(
        msg.contains("update_by is not implemented"),
        "message: {}",
        msg
    );
}

#[tokio::test(flavor = "current_thread")]
async fn delete_unique_returns_204_no_content_when_exactly_one_row_matches() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("email", Value::String("a@x".to_string())),
    ])]);
    let router = make_mutable_router(
        svc.clone(),
        "email",
        true,
        vec![deterministic::routes::ByFieldMethod::Delete],
        None,
    );
    let (status, body) = send(router, Method::DELETE, "/api/users/email/a@x", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert!(svc.all_rows().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn delete_unique_returns_404_when_no_rows_match() {
    let svc = Arc::new(MutableByFieldService::new());
    let router = make_mutable_router(
        svc,
        "email",
        true,
        vec![deterministic::routes::ByFieldMethod::Delete],
        None,
    );
    let (status, body) = send(router, Method::DELETE, "/api/users/email/missing", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}

#[tokio::test(flavor = "current_thread")]
async fn delete_unique_returns_409_when_multiple_rows_match() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![
        row(&[
            ("id", Value::from(1i64)),
            ("email", Value::String("dup@x".to_string())),
        ]),
        row(&[
            ("id", Value::from(2i64)),
            ("email", Value::String("dup@x".to_string())),
        ]),
    ]);
    svc.set_delete_count(2);
    let router = make_mutable_router(
        svc,
        "email",
        true,
        vec![deterministic::routes::ByFieldMethod::Delete],
        None,
    );
    let (status, body) = send(router, Method::DELETE, "/api/users/email/dup@x", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error_envelope(&body, "CONFLICT");
}

#[tokio::test(flavor = "current_thread")]
async fn delete_non_unique_returns_200_with_count_of_deleted_rows() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![
        row(&[
            ("id", Value::from(1i64)),
            ("phone", Value::String("555".to_string())),
        ]),
        row(&[
            ("id", Value::from(2i64)),
            ("phone", Value::String("555".to_string())),
        ]),
    ]);
    let router = make_mutable_router(
        svc,
        "phone",
        false,
        vec![deterministic::routes::ByFieldMethod::Delete],
        None,
    );
    let (status, body) = send(router, Method::DELETE, "/api/users/phone/555", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "count": 2 }));
}

#[tokio::test(flavor = "current_thread")]
async fn delete_service_receives_field_and_value_from_the_url() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![row(&[("email", Value::String("a@x".to_string()))])]);
    let router = make_mutable_router(
        svc.clone(),
        "email",
        false,
        vec![deterministic::routes::ByFieldMethod::Delete],
        None,
    );
    let (status, _) = send(router, Method::DELETE, "/api/users/email/a@x", None).await;
    assert_eq!(status, StatusCode::OK);
    let (field, value) = svc.last_delete_call().unwrap();
    assert_eq!(field, "email");
    assert_eq!(value, Value::String("a@x".to_string()));
}

#[tokio::test(flavor = "current_thread")]
async fn delete_returns_500_when_default_delete_by_impl_is_used() {
    let svc: Arc<MapByFieldService> = Arc::new(MapByFieldService::new());
    let router = create_by_field_router(ByFieldRouterConfig {
        service: svc,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        field: "email".to_string(),
        unique: false,
        methods: vec![deterministic::routes::ByFieldMethod::Delete],
        coerce_row: None,
        update_validator: None,
    });
    let (status, body) = send(router, Method::DELETE, "/api/users/email/a@x", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_error_envelope(&body, "INTERNAL_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(
        msg.contains("delete_by is not implemented"),
        "message: {}",
        msg
    );
}

#[tokio::test(flavor = "current_thread")]
async fn methods_get_only_returns_405_for_put_and_delete() {
    let svc = Arc::new(MutableByFieldService::new());
    let router = make_mutable_router(
        svc,
        "email",
        false,
        vec![deterministic::routes::ByFieldMethod::Get],
        None,
    );
    let (put_status, _) = send(
        router.clone(),
        Method::PUT,
        "/api/users/email/a@x",
        Some(json!({})),
    )
    .await;
    assert_eq!(put_status, StatusCode::METHOD_NOT_ALLOWED);
    let (del_status, _) = send(router, Method::DELETE, "/api/users/email/a@x", None).await;
    assert_eq!(del_status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn methods_empty_list_returns_405_for_all_verbs() {
    let svc = Arc::new(MutableByFieldService::new());
    let router = make_mutable_router(svc, "email", false, vec![], None);
    let (get_status, _) = send(router.clone(), Method::GET, "/api/users/email/a@x", None).await;
    assert_eq!(get_status, StatusCode::METHOD_NOT_ALLOWED);
    let (put_status, _) = send(router, Method::PUT, "/api/users/email/a@x", Some(json!({}))).await;
    assert_eq!(put_status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn methods_full_get_put_delete_all_mount_at_same_path() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("email", Value::String("full@x".to_string())),
    ])]);
    let router = make_mutable_router(
        svc.clone(),
        "email",
        true,
        vec![
            deterministic::routes::ByFieldMethod::Get,
            deterministic::routes::ByFieldMethod::Put,
            deterministic::routes::ByFieldMethod::Delete,
        ],
        None,
    );
    let (get_status, get_body) =
        send(router.clone(), Method::GET, "/api/users/email/full@x", None).await;
    assert_eq!(get_status, StatusCode::OK);
    assert_eq!(get_body.get("id").unwrap(), &json!(1));
    let (put_status, _) = send(
        router.clone(),
        Method::PUT,
        "/api/users/email/full@x",
        Some(json!({ "name": "z" })),
    )
    .await;
    assert_eq!(put_status, StatusCode::OK);
    let (del_status, _) = send(router, Method::DELETE, "/api/users/email/full@x", None).await;
    assert_eq!(del_status, StatusCode::NO_CONTENT);
}

#[tokio::test(flavor = "current_thread")]
async fn methods_duplicates_are_deduped_and_no_double_mount_panic() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.seed(vec![row(&[
        ("id", Value::from(1i64)),
        ("email", Value::String("dup@x".to_string())),
    ])]);
    let router = make_mutable_router(
        svc,
        "email",
        true,
        vec![
            deterministic::routes::ByFieldMethod::Get,
            deterministic::routes::ByFieldMethod::Get,
            deterministic::routes::ByFieldMethod::Get,
        ],
        None,
    );
    let (status, body) = send(router, Method::GET, "/api/users/email/dup@x", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(1));
}

#[tokio::test(flavor = "current_thread")]
async fn put_unique_returns_500_when_service_returns_count_1_but_find_by_returns_empty() {
    let svc = Arc::new(MutableByFieldService::new());
    svc.set_update_count(1);
    let router = make_mutable_router(
        svc,
        "email",
        true,
        vec![deterministic::routes::ByFieldMethod::Put],
        None,
    );
    let (status, body) = send(
        router,
        Method::PUT,
        "/api/users/email/ghost@x",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error_envelope(&body, "NOT_FOUND");
}
