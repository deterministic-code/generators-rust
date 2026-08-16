use axum::extract::Request;
use axum::http::{header, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

const HSTS: &str = "max-age=15552000; includeSubDomains";
const REFERRER_POLICY: &str = "no-referrer";
const X_FRAME_OPTIONS: &str = "SAMEORIGIN";
const X_CONTENT_TYPE_OPTIONS: &str = "nosniff";
const X_XSS_PROTECTION: &str = "0";
const X_DNS_PREFETCH_CONTROL: &str = "off";
const X_DOWNLOAD_OPTIONS: &str = "noopen";
const X_PERMITTED_CROSS_DOMAIN_POLICIES: &str = "none";
const CROSS_ORIGIN_OPENER_POLICY: &str = "same-origin";
const CROSS_ORIGIN_RESOURCE_POLICY: &str = "same-origin";
const ORIGIN_AGENT_CLUSTER: &str = "?1";

const CSP_DEFAULT: &str = "default-src 'self';base-uri 'self';font-src 'self' https: data:;form-action 'self';frame-ancestors 'self';img-src 'self' data:;object-src 'none';script-src 'self';script-src-attr 'none';style-src 'self' https: 'unsafe-inline';upgrade-insecure-requests";

const CSP_DOCS: &str = "default-src 'self';script-src 'self' blob:;worker-src 'self' blob:;style-src 'self' 'unsafe-inline';font-src 'self' data:;img-src 'self' data:";

pub struct SecurityHeadersMiddleware;

impl SecurityHeadersMiddleware {
    pub fn new() -> Self {
        Self
    }

    pub fn into_layer(
        self,
    ) -> axum::middleware::FromFnLayer<
        fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>,
        (),
        (),
    > {
        axum::middleware::from_fn(security_headers_layer_boxed)
    }
}

impl Default for SecurityHeadersMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

fn security_headers_layer_boxed(
    req: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(security_headers_layer(req, next))
}

pub async fn security_headers_layer(req: Request, next: Next) -> Response {
    let is_docs = req.uri().path().starts_with("/api/docs");
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    set_static(headers, header::STRICT_TRANSPORT_SECURITY, HSTS);
    set_static(headers, header::REFERRER_POLICY, REFERRER_POLICY);
    set_static(headers, header::X_FRAME_OPTIONS, X_FRAME_OPTIONS);
    set_static(
        headers,
        header::X_CONTENT_TYPE_OPTIONS,
        X_CONTENT_TYPE_OPTIONS,
    );
    set_static(headers, header::X_XSS_PROTECTION, X_XSS_PROTECTION);
    set_named(headers, "x-dns-prefetch-control", X_DNS_PREFETCH_CONTROL);
    set_named(headers, "x-download-options", X_DOWNLOAD_OPTIONS);
    set_named(
        headers,
        "x-permitted-cross-domain-policies",
        X_PERMITTED_CROSS_DOMAIN_POLICIES,
    );
    set_named(
        headers,
        "cross-origin-opener-policy",
        CROSS_ORIGIN_OPENER_POLICY,
    );
    set_named(
        headers,
        "cross-origin-resource-policy",
        CROSS_ORIGIN_RESOURCE_POLICY,
    );
    set_named(headers, "origin-agent-cluster", ORIGIN_AGENT_CLUSTER);
    let csp = if is_docs { CSP_DOCS } else { CSP_DEFAULT };
    set_static(headers, header::CONTENT_SECURITY_POLICY, csp);
    response
}

fn set_static(headers: &mut axum::http::HeaderMap, name: HeaderName, value: &'static str) {
    headers.insert(name, HeaderValue::from_static(value));
}

fn set_named(headers: &mut axum::http::HeaderMap, name: &'static str, value: &'static str) {
    headers.insert(
        HeaderName::from_static(name),
        HeaderValue::from_static(value),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/x", get(|| async { "ok" }))
            .route("/api/docs", get(|| async { "docs" }))
            .layer(axum::middleware::from_fn(security_headers_layer))
    }

    #[tokio::test]
    async fn sets_x_frame_options_sameorigin() {
        let req = HttpRequest::builder()
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::X_FRAME_OPTIONS).unwrap(),
            "SAMEORIGIN"
        );
    }

    #[tokio::test]
    async fn sets_x_content_type_options_nosniff() {
        let req = HttpRequest::builder()
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(
            res.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
            "nosniff"
        );
    }

    #[tokio::test]
    async fn sets_strict_transport_security() {
        let req = HttpRequest::builder()
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        let hsts = res
            .headers()
            .get(header::STRICT_TRANSPORT_SECURITY)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(hsts.contains("max-age="), "{}", hsts);
        assert!(hsts.contains("includeSubDomains"), "{}", hsts);
    }

    #[tokio::test]
    async fn default_csp_on_non_docs_path() {
        let req = HttpRequest::builder()
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        let csp = res
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("default-src 'self'"), "{}", csp);
        assert!(
            !csp.contains("blob:"),
            "non-docs path should not allow blob: in csp: {}",
            csp
        );
    }

    #[tokio::test]
    async fn relaxed_csp_on_api_docs_path() {
        let req = HttpRequest::builder()
            .uri("/api/docs")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        let csp = res
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            csp.contains("blob:"),
            "docs path should allow blob:: {}",
            csp
        );
        assert!(csp.contains("'unsafe-inline'"), "{}", csp);
    }
}
