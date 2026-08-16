use axum::{body::Body, http::Request, http::StatusCode, Router};
use deterministic::app::health;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_returns_200_with_status_ok_body() {
    let app: Router = Router::new().merge(health::router());

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body, json!({ "status": "ok" }));
}

#[tokio::test]
async fn health_endpoint_returns_404_for_unknown_path() {
    let app: Router = Router::new().merge(health::router());

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/not-health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
