use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use http_body_util::BodyExt;
use std::any::Any;
use tower_http::catch_panic::ResponseForPanic;

pub struct ErrorHandlerMiddleware;

impl ErrorHandlerMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ErrorHandlerMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn envelope_error_layer(req: Request, next: Next) -> Response {
    let response = next.run(req).await;
    let status = response.status();
    if !status.is_server_error() {
        return response;
    }
    if is_json_content_type(response.headers()) {
        let (parts, body) = response.into_parts();
        let bytes = match body.collect().await {
            Ok(c) => c.to_bytes(),
            Err(_) => return internal_error_envelope(),
        };
        if body_is_envelope(&bytes) {
            return Response::from_parts(parts, Body::from(bytes));
        }
        return internal_error_envelope();
    }
    internal_error_envelope()
}

fn is_json_content_type(headers: &HeaderMap<HeaderValue>) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.starts_with("application/json"))
        .unwrap_or(false)
}

fn body_is_envelope(bytes: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    v.get("errors")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .all(|e| e.get("code").is_some() && e.get("message").is_some())
        })
        .unwrap_or(false)
}

fn internal_error_envelope() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "errors": [{ "code": "INTERNAL_ERROR", "message": "Internal server error" }]
        })),
    )
        .into_response()
}

#[derive(Clone)]
pub struct EnvelopePanicResponder;

impl ResponseForPanic for EnvelopePanicResponder {
    type ResponseBody = Body;

    fn response_for_panic(
        &mut self,
        _err: Box<dyn Any + Send + 'static>,
    ) -> Response<Self::ResponseBody> {
        internal_error_envelope()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn json_body(res: Response) -> serde_json::Value {
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn layer_rewrites_empty_500_to_envelope() {
        let app: Router = Router::new()
            .route("/x", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
            .layer(axum::middleware::from_fn(envelope_error_layer));
        let req = Request::builder().uri("/x").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let v = json_body(res).await;
        assert_eq!(
            v,
            serde_json::json!({
                "errors": [{ "code": "INTERNAL_ERROR", "message": "Internal server error" }]
            })
        );
    }

    #[tokio::test]
    async fn layer_leaves_2xx_untouched() {
        let app: Router = Router::new()
            .route("/x", get(|| async { "hello" }))
            .layer(axum::middleware::from_fn(envelope_error_layer));
        let req = Request::builder().uri("/x").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes.as_ref(), b"hello");
    }

    #[tokio::test]
    async fn layer_preserves_already_envelope_5xx_body() {
        let app: Router = Router::new()
            .route(
                "/x",
                get(|| async {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "errors": [
                                { "code": "CUSTOM_500", "message": "already shaped" }
                            ]
                        })),
                    )
                }),
            )
            .layer(axum::middleware::from_fn(envelope_error_layer));
        let req = Request::builder().uri("/x").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        let v = json_body(res).await;
        assert_eq!(v["errors"][0]["code"], "CUSTOM_500");
        assert_eq!(v["errors"][0]["message"], "already shaped");
    }

    #[tokio::test]
    async fn layer_rewrites_non_envelope_json_5xx() {
        let app: Router = Router::new()
            .route(
                "/x",
                get(|| async {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "unrelated": true })),
                    )
                }),
            )
            .layer(axum::middleware::from_fn(envelope_error_layer));
        let req = Request::builder().uri("/x").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        let v = json_body(res).await;
        assert_eq!(v["errors"][0]["code"], "INTERNAL_ERROR");
    }

    #[tokio::test]
    async fn layer_rewrites_plain_text_5xx() {
        let app: Router = Router::new()
            .route(
                "/x",
                get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "boom") }),
            )
            .layer(axum::middleware::from_fn(envelope_error_layer));
        let req = Request::builder().uri("/x").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        let v = json_body(res).await;
        assert_eq!(v["errors"][0]["code"], "INTERNAL_ERROR");
        assert_eq!(v["errors"][0]["message"], "Internal server error");
    }

    #[tokio::test]
    async fn layer_leaves_4xx_untouched() {
        let app: Router = Router::new()
            .route(
                "/x",
                get(|| async { (StatusCode::BAD_REQUEST, "bad input") }),
            )
            .layer(axum::middleware::from_fn(envelope_error_layer));
        let req = Request::builder().uri("/x").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes.as_ref(), b"bad input");
    }
}
