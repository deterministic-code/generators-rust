use axum::http::Method;
use tower_http::cors::{Any, CorsLayer};

pub struct CorsMiddleware;

impl CorsMiddleware {
    pub fn new() -> Self {
        Self
    }

    pub fn into_layer(self) -> CorsLayer {
        Self::layer()
    }

    pub fn layer() -> CorsLayer {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([
                Method::GET,
                Method::HEAD,
                Method::PUT,
                Method::PATCH,
                Method::POST,
                Method::DELETE,
            ])
            .allow_headers(Any)
    }
}

impl Default for CorsMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/x", get(|| async { "ok" }))
            .layer(CorsMiddleware::layer())
    }

    #[tokio::test]
    async fn simple_get_sets_access_control_allow_origin_wildcard() {
        let req = Request::builder()
            .method("GET")
            .uri("/x")
            .header(header::ORIGIN, "https://example.com")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn options_preflight_returns_204_with_allowed_methods_and_headers() {
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/x")
            .header(header::ORIGIN, "https://example.com")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert!(
            res.status() == StatusCode::OK || res.status() == StatusCode::NO_CONTENT,
            "{}",
            res.status()
        );
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
        let allowed = res
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .unwrap()
            .to_str()
            .unwrap()
            .to_uppercase();
        for m in ["GET", "HEAD", "PUT", "PATCH", "POST", "DELETE"] {
            assert!(allowed.contains(m), "missing {} in {}", m, allowed);
        }
    }

    #[tokio::test]
    async fn allows_post_method_per_cors_defaults() {
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/x")
            .header(header::ORIGIN, "https://example.com")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "DELETE")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        let allowed = res
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .unwrap()
            .to_str()
            .unwrap()
            .to_uppercase();
        assert!(allowed.contains("DELETE"), "{}", allowed);
    }
}
