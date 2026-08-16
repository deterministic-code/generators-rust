use tower_http::limit::RequestBodyLimitLayer;

pub const FORM_BODY_LIMIT_BYTES: usize = 100 * 1024;

pub struct FormBodyMiddleware {
    limit_bytes: usize,
}

impl FormBodyMiddleware {
    pub fn new() -> Self {
        Self {
            limit_bytes: FORM_BODY_LIMIT_BYTES,
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

impl Default for FormBodyMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_nested<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, serde_qs::Error> {
    serde_qs::from_str(input)
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
    use serde::Deserialize;
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
            .layer(FormBodyMiddleware::layer())
    }

    #[tokio::test]
    async fn body_under_default_limit_is_allowed() {
        let body = vec![b'a'; 1024];
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/x")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn body_over_default_limit_is_rejected() {
        let body = vec![b'a'; FORM_BODY_LIMIT_BYTES + 1];
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/x")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn default_limit_is_one_hundred_kilobytes() {
        assert_eq!(FORM_BODY_LIMIT_BYTES, 102_400);
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct Nested {
        foo: Inner,
    }
    #[derive(Deserialize, Debug, PartialEq)]
    struct Inner {
        bar: String,
    }

    #[test]
    fn parse_nested_decodes_bracketed_key_syntax() {
        let parsed: Nested = parse_nested("foo[bar]=baz").expect("parse");
        assert_eq!(
            parsed,
            Nested {
                foo: Inner {
                    bar: "baz".to_string()
                }
            }
        );
    }

    #[test]
    fn parse_nested_decodes_deeply_nested_form() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Outer {
            a: Mid,
        }
        #[derive(Deserialize, Debug, PartialEq)]
        struct Mid {
            b: Inner2,
        }
        #[derive(Deserialize, Debug, PartialEq)]
        struct Inner2 {
            c: String,
        }
        let parsed: Outer = parse_nested("a[b][c]=hello").expect("parse");
        assert_eq!(parsed.a.b.c, "hello");
    }
}
