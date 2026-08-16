use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use deterministic::routes::{iterate_combined_routers, mount_combined_routes};

async fn hit(app: Router, uri: &str) -> (StatusCode, Value) {
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

#[tokio::test(flavor = "current_thread")]
async fn mount_combined_routes_returns_an_empty_router_when_input_is_empty() {
    let router = mount_combined_routes(Vec::new());
    let (status, _) = hit(router, "/anywhere").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "current_thread")]
async fn mount_combined_routes_serves_routes_from_a_single_input_router() {
    let one = Router::new().route(
        "/api/one",
        get(|| async { axum::Json(json!({ "value": 1 })) }),
    );
    let router = mount_combined_routes(vec![one]);
    let (status, body) = hit(router, "/api/one").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "value": 1 }));
}

#[tokio::test(flavor = "current_thread")]
async fn mount_combined_routes_serves_routes_from_all_input_routers() {
    let one = Router::new().route("/api/one", get(|| async { axum::Json(json!({ "n": 1 })) }));
    let two = Router::new().route("/api/two", get(|| async { axum::Json(json!({ "n": 2 })) }));
    let three = Router::new().route(
        "/api/three",
        get(|| async { axum::Json(json!({ "n": 3 })) }),
    );
    let router = mount_combined_routes(vec![one, two, three]);
    let (one_status, one_body) = hit(router.clone(), "/api/one").await;
    assert_eq!(one_status, StatusCode::OK);
    assert_eq!(one_body, json!({ "n": 1 }));
    let (two_status, two_body) = hit(router.clone(), "/api/two").await;
    assert_eq!(two_status, StatusCode::OK);
    assert_eq!(two_body, json!({ "n": 2 }));
    let (three_status, three_body) = hit(router, "/api/three").await;
    assert_eq!(three_status, StatusCode::OK);
    assert_eq!(three_body, json!({ "n": 3 }));
}

#[tokio::test(flavor = "current_thread")]
async fn mount_combined_routes_returns_404_for_paths_not_present_in_any_input_router() {
    let one = Router::new().route(
        "/api/present",
        get(|| async { axum::Json(json!({ "ok": true })) }),
    );
    let router = mount_combined_routes(vec![one]);
    let (status, _) = hit(router, "/api/missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "current_thread")]
async fn iterate_combined_routers_yields_every_router_from_vec_input() {
    let inputs = vec![Router::new(), Router::new(), Router::new()];
    let out: Vec<Router> = iterate_combined_routers(inputs).collect();
    assert_eq!(out.len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn iterate_combined_routers_yields_nothing_for_empty_input() {
    let empty: Vec<Router> = Vec::new();
    let out: Vec<Router> = iterate_combined_routers(empty).collect();
    assert!(out.is_empty());
}
