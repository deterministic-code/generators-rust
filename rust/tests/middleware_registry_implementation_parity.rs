use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use deterministic::middleware::{
    security_headers_layer, AppError, CorsMiddleware, ErrorHandler, FormBodyMiddleware,
    JsonBodyMiddleware, SecurityHeadersMiddleware,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[test]
fn registered_app_tier_names_all_have_concrete_rust_types() {
    let _cors = CorsMiddleware::new();
    let _sec = SecurityHeadersMiddleware::new();
    let _json = JsonBodyMiddleware::new();
    let _form = FormBodyMiddleware::new();
    let _err = ErrorHandler::new();
}

#[tokio::test]
async fn cors_middleware_observes_ts_default_allow_origin_wildcard() {
    let app: Router = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(CorsMiddleware::layer());
    let req = Request::builder()
        .uri("/x")
        .header(header::ORIGIN, "https://example.com")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "*",
        "TS cors() default emits Access-Control-Allow-Origin: *"
    );
}

#[tokio::test]
async fn security_headers_pins_x_frame_options_x_content_type_options_csp() {
    let app: Router = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn(security_headers_layer));
    let req = Request::builder().uri("/x").body(Body::empty()).unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(
        res.headers().get(header::X_FRAME_OPTIONS).unwrap(),
        "SAMEORIGIN"
    );
    assert_eq!(
        res.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff"
    );
    assert!(res.headers().contains_key(header::CONTENT_SECURITY_POLICY));
}

#[tokio::test]
async fn json_body_middleware_rejects_over_1mb_to_match_ts_express_json_limit() {
    use axum::extract::Request as ExReq;
    async fn echo(req: ExReq) -> Result<String, StatusCode> {
        match req.into_body().collect().await {
            Ok(c) => Ok(c.to_bytes().len().to_string()),
            Err(_) => Err(StatusCode::PAYLOAD_TOO_LARGE),
        }
    }
    let app: Router = Router::new()
        .route("/x", post(echo))
        .layer(JsonBodyMiddleware::layer());
    let body = vec![b'a'; 1_048_577];
    let req = Request::builder()
        .method("POST")
        .uri("/x")
        .body(Body::from(body))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn error_handler_validation_envelope_matches_ts_send_error_shape() {
    use axum::response::IntoResponse;
    let res = AppError::Validation("name: required".to_string()).into_response();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let expected = serde_json::json!({
        "errors": [
            { "code": "VALIDATION_ERROR", "message": "name: required" }
        ]
    });
    assert_eq!(
        v, expected,
        "TS sendError shape: {{ errors: [{{ code, message }}] }}"
    );
}

#[tokio::test]
async fn error_handler_internal_envelope_matches_ts_500_internal_error_shape() {
    use axum::response::IntoResponse;
    let res = AppError::Internal.into_response();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "errors": [
                { "code": "INTERNAL_ERROR", "message": "Internal server error" }
            ]
        })
    );
}

#[tokio::test]
async fn form_body_middleware_rejects_over_100kb_to_match_ts_express_urlencoded_default() {
    use axum::extract::Request as ExReq;
    async fn echo(req: ExReq) -> Result<String, StatusCode> {
        match req.into_body().collect().await {
            Ok(c) => Ok(c.to_bytes().len().to_string()),
            Err(_) => Err(StatusCode::PAYLOAD_TOO_LARGE),
        }
    }
    let app: Router = Router::new()
        .route("/x", post(echo))
        .layer(FormBodyMiddleware::layer());
    let body = vec![b'a'; 102_401];
    let req = Request::builder()
        .method("POST")
        .uri("/x")
        .body(Body::from(body))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
