use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub struct NotFoundMiddleware;

impl NotFoundMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NotFoundMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn not_found_fallback() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "errors": [{ "code": "NOT_FOUND", "message": "Route not found" }]
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn fallback_returns_404_status() {
        let response = not_found_fallback().await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn fallback_returns_envelope_with_not_found_code_and_message() {
        let response = not_found_fallback().await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "errors": [{ "code": "NOT_FOUND", "message": "Route not found" }]
            })
        );
    }

    #[tokio::test]
    async fn fallback_envelope_has_exactly_one_key_errors() {
        let response = not_found_fallback().await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 1);
        assert!(v.get("errors").is_some());
    }

    #[tokio::test]
    async fn fallback_envelope_errors_array_has_exactly_one_entry() {
        let response = not_found_fallback().await;
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["errors"].as_array().unwrap().len(), 1);
    }
}
