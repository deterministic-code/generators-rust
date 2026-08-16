use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeDir;

async fn make_static_dir(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("det-b1-{}-{}", name, std::process::id()));
    fs::create_dir_all(&dir).await.unwrap();
    for (rel, contents) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.unwrap();
        }
        let mut f = fs::File::create(&path).await.unwrap();
        f.write_all(contents).await.unwrap();
    }
    dir
}

fn app(prefix: &str, dir: &PathBuf) -> Router {
    Router::new()
        .route("/api/live", get(|| async { "live" }))
        .nest_service(prefix, ServeDir::new(dir))
}

#[tokio::test]
async fn mounted_static_file_returns_200_with_body() {
    let dir = make_static_dir("mounted", &[("x.txt", b"hello statics")]).await;
    let app = app("/assets", &dir);
    let req = Request::builder()
        .uri("/assets/x.txt")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), b"hello statics");
    fs::remove_dir_all(dir).await.ok();
}

#[tokio::test]
async fn nested_static_file_resolves_relative_to_dir() {
    let dir = make_static_dir("nested", &[("sub/y.txt", b"nested body")]).await;
    let app = app("/assets", &dir);
    let req = Request::builder()
        .uri("/assets/sub/y.txt")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), b"nested body");
    fs::remove_dir_all(dir).await.ok();
}

#[tokio::test]
async fn missing_static_file_returns_404() {
    let dir = make_static_dir("missing", &[("y.txt", b"only y")]).await;
    let app = app("/assets", &dir);
    let req = Request::builder()
        .uri("/assets/does-not-exist.txt")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    fs::remove_dir_all(dir).await.ok();
}

#[tokio::test]
async fn api_route_takes_precedence_over_matching_static_prefix() {
    let dir = make_static_dir("precedence", &[("live", b"static live")]).await;
    let app = app("/api", &dir);
    let req = Request::builder()
        .uri("/api/live")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), b"live");
    fs::remove_dir_all(dir).await.ok();
}
