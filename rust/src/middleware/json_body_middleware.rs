use tower_http::limit::RequestBodyLimitLayer;

pub const JSON_BODY_LIMIT_BYTES: usize = 1024 * 1024;

pub struct JsonBodyMiddleware {
    limit_bytes: usize,
}

impl JsonBodyMiddleware {
    pub fn new() -> Self {
        Self {
            limit_bytes: JSON_BODY_LIMIT_BYTES,
        }
    }

    pub fn with_limit_bytes(limit_bytes: usize) -> Self {
        Self { limit_bytes }
    }

    pub fn into_layer(self) -> RequestBodyLimitLayer {
        RequestBodyLimitLayer::new(self.limit_bytes)
    }

    pub fn layer() -> RequestBodyLimitLayer {
        Self::new().into_layer()
    }
}

impl Default for JsonBodyMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn echo_byte_count(req: Request) -> Result<String, StatusCode> {
        match req.into_body().collect().await {
            Ok(c) => Ok(c.to_bytes().len().to_string()),
            Err(_) => Err(StatusCode::PAYLOAD_TOO_LARGE),
        }
    }

    fn app() -> Router {
        Router::new()
            .route("/x", post(echo_byte_count))
            .layer(JsonBodyMiddleware::layer())
    }

    #[tokio::test]
    async fn body_under_limit_is_allowed() {
        let body = vec![b'a'; 1024];
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/x")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn body_over_one_mb_is_rejected() {
        let body = vec![b'a'; JSON_BODY_LIMIT_BYTES + 1];
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/x")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn limit_default_is_one_megabyte() {
        assert_eq!(JSON_BODY_LIMIT_BYTES, 1_048_576);
    }

    #[tokio::test]
    async fn custom_limit_overrides_default() {
        let small = JsonBodyMiddleware::with_limit_bytes(8);
        let app = Router::new()
            .route("/x", post(echo_byte_count))
            .layer(small.into_layer());
        let body = vec![b'a'; 16];
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/x")
            .body(Body::from(body))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
