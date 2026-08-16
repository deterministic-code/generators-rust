use tower_http::limit::RequestBodyLimitLayer;

pub const LARGE_JSON_BODY_LIMIT_BYTES: usize = 50 * 1024 * 1024;

pub struct LargeJsonBodyMiddleware;

impl LargeJsonBodyMiddleware {
    pub fn layer() -> RequestBodyLimitLayer {
        RequestBodyLimitLayer::new(LARGE_JSON_BODY_LIMIT_BYTES)
    }
}
