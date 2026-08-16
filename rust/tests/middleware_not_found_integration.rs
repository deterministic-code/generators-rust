use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use deterministic::middleware::not_found_fallback;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app() -> Router {
    Router::new()
        .route("/matched", get(|| async { "hello" }))
        .fallback(not_found_fallback)
}

async fn body_json(res: axum::response::Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn unmatched_get_returns_404_with_envelope() {
    let req = Request::builder()
        .method("GET")
        .uri("/nope")
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let v = body_json(res).await;
    assert_eq!(
        v,
        serde_json::json!({
            "errors": [{ "code": "NOT_FOUND", "message": "Route not found" }]
        })
    );
}

#[tokio::test]
async fn unmatched_post_returns_404_with_envelope() {
    let app = Router::new()
        .route("/matched", post(|| async { "hello" }))
        .fallback(not_found_fallback);
    let req = Request::builder()
        .method("POST")
        .uri("/nope")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let v = body_json(res).await;
    assert_eq!(v["errors"][0]["code"], "NOT_FOUND");
}

#[tokio::test]
async fn unmatched_patch_returns_404_with_envelope() {
    let app = Router::new()
        .route("/matched", patch(|| async { "hello" }))
        .fallback(not_found_fallback);
    let req = Request::builder()
        .method("PATCH")
        .uri("/nope")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let v = body_json(res).await;
    assert_eq!(v["errors"][0]["code"], "NOT_FOUND");
}

#[tokio::test]
async fn unmatched_delete_returns_404_with_envelope() {
    let app = Router::new()
        .route("/matched", delete(|| async { "hello" }))
        .fallback(not_found_fallback);
    let req = Request::builder()
        .method("DELETE")
        .uri("/nope")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let v = body_json(res).await;
    assert_eq!(v["errors"][0]["code"], "NOT_FOUND");
}

#[tokio::test]
async fn matched_route_still_returns_200() {
    let req = Request::builder()
        .method("GET")
        .uri("/matched")
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn nested_path_segments_404_through_fallback() {
    let req = Request::builder()
        .method("GET")
        .uri("/api/does/not/exist/deeply/nested")
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let v = body_json(res).await;
    assert_eq!(v["errors"][0]["code"], "NOT_FOUND");
}

#[tokio::test]
async fn query_string_discarded_envelope_stays_static() {
    let req = Request::builder()
        .method("GET")
        .uri("/nope?foo=bar&x=1")
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    let v = body_json(res).await;
    assert_eq!(
        v,
        serde_json::json!({
            "errors": [{ "code": "NOT_FOUND", "message": "Route not found" }]
        })
    );
}

#[tokio::test]
async fn envelope_has_exactly_one_key_errors() {
    let req = Request::builder()
        .method("GET")
        .uri("/nope")
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    let v = body_json(res).await;
    assert_eq!(v.as_object().unwrap().len(), 1);
    assert!(v.get("errors").is_some());
}

#[tokio::test]
async fn envelope_errors_array_has_exactly_one_entry() {
    let req = Request::builder()
        .method("GET")
        .uri("/nope")
        .body(Body::empty())
        .unwrap();
    let res = app().oneshot(req).await.unwrap();
    let v = body_json(res).await;
    assert_eq!(v["errors"].as_array().unwrap().len(), 1);
}
