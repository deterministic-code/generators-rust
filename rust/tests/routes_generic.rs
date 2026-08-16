use std::sync::Arc;

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
    create_generic_router, GenericHandlerFn, GenericResponseFormat, GenericRouterConfig,
    GenericRouterMethod, ValidatorFn,
};

fn echo_body_handler() -> GenericHandlerFn {
    Arc::new(|body: RowMap| {
        Box::pin(async move {
            let v = Value::Object(body.into_iter().collect());
            Ok(v)
        })
    })
}

fn fixed_item_handler() -> GenericHandlerFn {
    Arc::new(|_body| Box::pin(async move { Ok(json!({ "status": "ok" })) }))
}

fn fixed_items_handler() -> GenericHandlerFn {
    Arc::new(|_body| Box::pin(async move { Ok(json!([{ "id": 1 }, { "id": 2 }])) }))
}

fn failing_handler(msg: &'static str) -> GenericHandlerFn {
    Arc::new(move |_body| {
        Box::pin(async move { Err(RepositoryError::UnsupportedQuery(msg.to_string())) })
    })
}

fn require_present_validator() -> ValidatorFn {
    Arc::new(|body: &RowMap| {
        if body.get("name").is_some() {
            Ok(())
        } else {
            Err(vec!["name: required".to_string()])
        }
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

fn cfg(
    method: GenericRouterMethod,
    handler: GenericHandlerFn,
    response_format: GenericResponseFormat,
) -> GenericRouterConfig {
    GenericRouterConfig {
        method,
        path: "/api/health".to_string(),
        handler,
        request_validator: None,
        response_format,
        status_code: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn get_with_item_format_returns_200_and_raw_body() {
    let router = create_generic_router(cfg(
        GenericRouterMethod::Get,
        fixed_item_handler(),
        GenericResponseFormat::Item,
    ));
    let (status, body) = send(router, Method::GET, "/api/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
}

#[tokio::test(flavor = "current_thread")]
async fn post_with_item_format_defaults_to_201() {
    let router = create_generic_router(cfg(
        GenericRouterMethod::Post,
        echo_body_handler(),
        GenericResponseFormat::Item,
    ));
    let (status, body) = send(
        router,
        Method::POST,
        "/api/health",
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body, json!({ "name": "x" }));
}

#[tokio::test(flavor = "current_thread")]
async fn post_with_items_format_stays_at_200() {
    let router = create_generic_router(cfg(
        GenericRouterMethod::Post,
        fixed_items_handler(),
        GenericResponseFormat::Items,
    ));
    let (status, body) = send(router, Method::POST, "/api/health", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "items": [{ "id": 1 }, { "id": 2 }] }));
}

#[tokio::test(flavor = "current_thread")]
async fn get_with_items_format_wraps_in_items_envelope() {
    let router = create_generic_router(cfg(
        GenericRouterMethod::Get,
        fixed_items_handler(),
        GenericResponseFormat::Items,
    ));
    let (status, body) = send(router, Method::GET, "/api/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "items": [{ "id": 1 }, { "id": 2 }] }));
}

#[tokio::test(flavor = "current_thread")]
async fn raw_format_returns_handler_value_verbatim() {
    let router = create_generic_router(cfg(
        GenericRouterMethod::Get,
        Arc::new(|_body| Box::pin(async move { Ok(json!({ "any": "shape", "no": "wrapping" })) })),
        GenericResponseFormat::Raw,
    ));
    let (status, body) = send(router, Method::GET, "/api/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "any": "shape", "no": "wrapping" }));
}

#[tokio::test(flavor = "current_thread")]
async fn put_and_patch_and_delete_are_routable() {
    for method in [
        (GenericRouterMethod::Put, Method::PUT),
        (GenericRouterMethod::Patch, Method::PATCH),
        (GenericRouterMethod::Delete, Method::DELETE),
    ] {
        let router = create_generic_router(cfg(
            method.0,
            fixed_item_handler(),
            GenericResponseFormat::Item,
        ));
        let (status, body) = send(router, method.1, "/api/health", None).await;
        assert_eq!(status, StatusCode::OK, "method: {:?}", method.0);
        assert_eq!(body, json!({ "status": "ok" }));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn method_mismatch_returns_405_method_not_allowed() {
    let router = create_generic_router(cfg(
        GenericRouterMethod::Get,
        fixed_item_handler(),
        GenericResponseFormat::Item,
    ));
    let (status, _) = send(router, Method::POST, "/api/health", Some(json!({}))).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test(flavor = "current_thread")]
async fn status_code_override_wins_over_default() {
    let mut config = cfg(
        GenericRouterMethod::Post,
        fixed_item_handler(),
        GenericResponseFormat::Item,
    );
    config.status_code = Some(202);
    let router = create_generic_router(config);
    let (status, _) = send(router, Method::POST, "/api/health", Some(json!({}))).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test(flavor = "current_thread")]
async fn request_validator_gates_the_handler_with_400_envelope() {
    let mut config = cfg(
        GenericRouterMethod::Post,
        echo_body_handler(),
        GenericResponseFormat::Item,
    );
    config.request_validator = Some(require_present_validator());
    let router = create_generic_router(config);
    let (status, body) = send(router, Method::POST, "/api/health", Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error_envelope(&body, "VALIDATION_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("name: required"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn request_validator_lets_valid_body_through() {
    let mut config = cfg(
        GenericRouterMethod::Post,
        echo_body_handler(),
        GenericResponseFormat::Item,
    );
    config.request_validator = Some(require_present_validator());
    let router = create_generic_router(config);
    let (status, body) = send(
        router,
        Method::POST,
        "/api/health",
        Some(json!({ "name": "alice" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.get("name").unwrap(), &json!("alice"));
}

#[tokio::test(flavor = "current_thread")]
async fn handler_error_returns_500_with_errors_envelope() {
    let router = create_generic_router(cfg(
        GenericRouterMethod::Get,
        failing_handler("handler blew up"),
        GenericResponseFormat::Item,
    ));
    let (status, body) = send(router, Method::GET, "/api/health", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_error_envelope(&body, "INTERNAL_ERROR");
    let msg = body["errors"][0]["message"].as_str().unwrap();
    assert!(msg.contains("handler blew up"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn handler_receives_empty_body_on_get_when_no_body_sent() {
    let router = create_generic_router(cfg(
        GenericRouterMethod::Get,
        echo_body_handler(),
        GenericResponseFormat::Item,
    ));
    let (status, body) = send(router, Method::GET, "/api/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({}));
}

#[tokio::test(flavor = "current_thread")]
async fn custom_route_still_works_when_merged_with_generic_router() {
    let router = Router::new()
        .merge(create_generic_router(cfg(
            GenericRouterMethod::Get,
            fixed_item_handler(),
            GenericResponseFormat::Item,
        )))
        .merge(custom_router());
    let (status, body) = send(router, Method::GET, "/api/custom-thing", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "custom": "yes" }));
}
