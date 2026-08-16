use axum::body::Body;
use axum::extract::Request;
use axum::http::{Request as HttpRequest, StatusCode};
use axum::routing::post;
use axum::Router;
use deterministic::middleware::{JsonBodyMiddleware, JSON_BODY_LIMIT_BYTES};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn echo(req: Request) -> Result<String, StatusCode> {
    match req.into_body().collect().await {
        Ok(c) => Ok(c.to_bytes().len().to_string()),
        Err(_) => Err(StatusCode::PAYLOAD_TOO_LARGE),
    }
}

#[tokio::test]
async fn json_body_middleware_layer_enforces_default_1mb_limit_via_public_api() {
    let app: Router = Router::new()
        .route("/x", post(echo))
        .layer(JsonBodyMiddleware::layer());

    let body = vec![b'a'; JSON_BODY_LIMIT_BYTES + 1];
    let req = HttpRequest::builder()
        .method("POST")
        .uri("/x")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn json_body_middleware_with_limit_bytes_overrides_default_via_public_api() {
    let app: Router = Router::new()
        .route("/x", post(echo))
        .layer(JsonBodyMiddleware::with_limit_bytes(4).into_layer());

    let body = vec![b'a'; 16];
    let req = HttpRequest::builder()
        .method("POST")
        .uri("/x")
        .body(Body::from(body))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn json_body_limit_constant_is_one_megabyte() {
    assert_eq!(JSON_BODY_LIMIT_BYTES, 1_048_576);
}
