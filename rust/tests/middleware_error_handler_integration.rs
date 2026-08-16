use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use deterministic::middleware::{
    envelope_error_layer, not_found_fallback, ApiErrorEntry, AppError, EnvelopePanicResponder,
    ErrorHandler,
};
use http_body_util::BodyExt;
use tower::ServiceExt;
use tower_http::catch_panic::CatchPanicLayer;

async fn body_bytes(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn app_error_validation_returns_400_envelope_via_public_api() {
    let res = AppError::Validation("missing field".to_string()).into_response();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_bytes(res).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["errors"][0]["code"].as_str().unwrap(), "VALIDATION_ERROR");
    assert_eq!(v["errors"][0]["message"].as_str().unwrap(), "missing field");
}

#[tokio::test]
async fn app_error_conflict_returns_409_envelope_via_public_api() {
    let res = AppError::Conflict("duplicate key".to_string()).into_response();
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body = body_bytes(res).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["errors"][0]["code"].as_str().unwrap(), "CONFLICT");
}

#[tokio::test]
async fn app_error_internal_returns_500_internal_error_envelope_via_public_api() {
    let res = AppError::Internal.into_response();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_bytes(res).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["errors"][0]["code"].as_str().unwrap(), "INTERNAL_ERROR");
    assert_eq!(
        v["errors"][0]["message"].as_str().unwrap(),
        "Internal server error"
    );
}

#[tokio::test]
async fn error_handler_envelope_helper_returns_byte_identical_to_app_error_custom() {
    let r1 = ErrorHandler::envelope(403, "FORBIDDEN", "nope");
    let r2 = AppError::Custom {
        status: 403,
        code: "FORBIDDEN".to_string(),
        message: "nope".to_string(),
    }
    .into_response();
    assert_eq!(r1.status(), r2.status());
    let b1 = body_bytes(r1).await;
    let b2 = body_bytes(r2).await;
    assert_eq!(b1, b2);
}

#[tokio::test]
async fn error_handler_envelope_many_returns_400_with_multiple_entries() {
    let res = ErrorHandler::envelope_many(
        400,
        vec![
            ApiErrorEntry {
                code: "VALIDATION_ERROR".to_string(),
                message: "email: required".to_string(),
            },
            ApiErrorEntry {
                code: "VALIDATION_ERROR".to_string(),
                message: "name: required".to_string(),
            },
        ],
    );
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_bytes(res).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["errors"].as_array().unwrap().len(), 2);
}

fn error_handled_router() -> Router {
    Router::new()
        .route("/matched", get(|| async { "hello" }))
        .route(
            "/validate",
            get(|| async { Err::<&str, _>(AppError::Validation("bad".to_string())) }),
        )
        .route(
            "/conflict",
            get(|| async { Err::<&str, _>(AppError::Conflict("dup".to_string())) }),
        )
        .route(
            "/precondition",
            get(|| async {
                Err::<&str, _>(AppError::Custom {
                    status: 428,
                    code: "PRECONDITION_REQUIRED".to_string(),
                    message: "if-match required".to_string(),
                })
            }),
        )
        .route(
            "/untyped",
            get(|| async { StatusCode::INTERNAL_SERVER_ERROR.into_response() }),
        )
        .route("/panic", get(panic_handler))
        .fallback(not_found_fallback)
        .layer(axum::middleware::from_fn(envelope_error_layer))
        .layer(CatchPanicLayer::custom(EnvelopePanicResponder))
}

async fn panic_handler() -> axum::response::Response {
    panic!("boom");
}

async fn json_body(res: axum::response::Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn call(uri: &str) -> axum::response::Response {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    error_handled_router().oneshot(req).await.unwrap()
}

#[tokio::test]
async fn validation_error_yields_400_envelope() {
    let res = call("/validate").await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let v = json_body(res).await;
    assert_eq!(v["errors"][0]["code"], "VALIDATION_ERROR");
    assert_eq!(v["errors"][0]["message"], "bad");
}

#[tokio::test]
async fn conflict_error_yields_409_envelope() {
    let res = call("/conflict").await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let v = json_body(res).await;
    assert_eq!(v["errors"][0]["code"], "CONFLICT");
    assert_eq!(v["errors"][0]["message"], "dup");
}

#[tokio::test]
async fn precondition_error_yields_428_envelope() {
    let res = call("/precondition").await;
    assert_eq!(res.status(), StatusCode::PRECONDITION_REQUIRED);
    let v = json_body(res).await;
    assert_eq!(v["errors"][0]["code"], "PRECONDITION_REQUIRED");
    assert_eq!(v["errors"][0]["message"], "if-match required");
}

#[tokio::test]
async fn untyped_500_becomes_internal_error_envelope() {
    let res = call("/untyped").await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let v = json_body(res).await;
    assert_eq!(v["errors"][0]["code"], "INTERNAL_ERROR");
    assert_eq!(v["errors"][0]["message"], "Internal server error");
}

#[tokio::test]
async fn two_hundred_untouched() {
    let res = call("/matched").await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), b"hello");
}

#[tokio::test]
async fn panic_becomes_500_envelope_via_catch_panic() {
    let res = call("/panic").await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let v = json_body(res).await;
    assert_eq!(v["errors"][0]["code"], "INTERNAL_ERROR");
    assert_eq!(v["errors"][0]["message"], "Internal server error");
}

#[tokio::test]
async fn composed_with_not_found_unrouted_returns_not_found() {
    let res = call("/no-such-path").await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let v = json_body(res).await;
    assert_eq!(v["errors"][0]["code"], "NOT_FOUND");
}

#[tokio::test]
async fn composed_with_not_found_routed_error_returns_internal_error() {
    let res = call("/untyped").await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let v = json_body(res).await;
    assert_eq!(v["errors"][0]["code"], "INTERNAL_ERROR");
}
