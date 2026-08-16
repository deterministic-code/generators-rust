use axum::body::Body;
use axum::extract::Request;
use axum::http::{Request as HttpRequest, StatusCode};
use axum::routing::post;
use axum::Router;
use deterministic::middleware::form_body_middleware::parse_nested;
use deterministic::middleware::{FormBodyMiddleware, FORM_BODY_LIMIT_BYTES};
use http_body_util::BodyExt;
use serde::Deserialize;
use tower::ServiceExt;

async fn echo(req: Request) -> Result<String, StatusCode> {
    match req.into_body().collect().await {
        Ok(c) => Ok(c.to_bytes().len().to_string()),
        Err(_) => Err(StatusCode::PAYLOAD_TOO_LARGE),
    }
}

#[tokio::test]
async fn form_body_middleware_layer_enforces_default_100kb_limit_via_public_api() {
    let app: Router = Router::new()
        .route("/x", post(echo))
        .layer(FormBodyMiddleware::layer());

    let body = vec![b'a'; FORM_BODY_LIMIT_BYTES + 1];
    let req = HttpRequest::builder()
        .method("POST")
        .uri("/x")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn parse_nested_via_public_api_decodes_bracketed_key_form() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Wrapper {
        user: User,
    }
    #[derive(Deserialize, Debug, PartialEq)]
    struct User {
        name: String,
        age: u32,
    }
    let parsed: Wrapper = parse_nested("user[name]=alice&user[age]=30").expect("parse");
    assert_eq!(parsed.user.name, "alice");
    assert_eq!(parsed.user.age, 30);
}
