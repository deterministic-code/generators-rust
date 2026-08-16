use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use deterministic::middleware::security_headers_layer;
use tower::ServiceExt;

fn app() -> Router {
    Router::new()
        .route("/x", get(|| async { "ok" }))
        .route("/api/docs", get(|| async { "docs" }))
        .layer(axum::middleware::from_fn(security_headers_layer))
}

#[tokio::test]
async fn security_headers_layer_sets_baseline_security_headers_via_public_api() {
    let req = Request::builder().uri("/x").body(Body::empty()).unwrap();
    let res = app().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::X_FRAME_OPTIONS).unwrap(),
        "SAMEORIGIN"
    );
    assert_eq!(
        res.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff"
    );
    assert!(res.headers().contains_key(header::CONTENT_SECURITY_POLICY));
    assert!(res
        .headers()
        .contains_key(header::STRICT_TRANSPORT_SECURITY));
}

#[tokio::test]
async fn security_headers_layer_relaxes_csp_on_api_docs_path() {
    let req = Request::builder()
        .uri("/api/docs")
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    let csp = res
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("blob:"), "{}", csp);
    assert!(csp.contains("'unsafe-inline'"), "{}", csp);
}
