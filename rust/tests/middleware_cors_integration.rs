use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use deterministic::middleware::CorsMiddleware;
use tower::ServiceExt;

#[tokio::test]
async fn cors_middleware_layer_sets_access_control_allow_origin_via_public_api() {
    let app: Router = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(CorsMiddleware::layer());

    let req = Request::builder()
        .method("GET")
        .uri("/x")
        .header(header::ORIGIN, "https://example.com")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "*"
    );
}

#[tokio::test]
async fn cors_middleware_into_layer_handles_options_preflight_via_public_api() {
    let mw = CorsMiddleware::new();
    let app: Router = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(mw.into_layer());

    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri("/x")
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "DELETE")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    let allowed = res
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .unwrap()
        .to_str()
        .unwrap()
        .to_uppercase();
    for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"] {
        assert!(allowed.contains(m), "missing {} in {}", m, allowed);
    }
}
