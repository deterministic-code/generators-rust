use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiErrorEntry {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum AppError {
    Validation(String),
    Conflict(String),
    NotFound(String),
    ForeignKey(String),
    Business {
        status_code: u16,
        code: String,
        message: String,
    },
    Custom {
        status: u16,
        code: String,
        message: String,
    },
    ValidationMany(Vec<ApiErrorEntry>),
    Internal,
}

impl AppError {
    fn into_parts(self) -> (StatusCode, Vec<ApiErrorEntry>) {
        match self {
            AppError::Validation(message) => (
                StatusCode::BAD_REQUEST,
                vec![ApiErrorEntry {
                    code: "VALIDATION_ERROR".to_string(),
                    message,
                }],
            ),
            AppError::ValidationMany(entries) => (StatusCode::BAD_REQUEST, entries),
            AppError::Conflict(message) => (
                StatusCode::CONFLICT,
                vec![ApiErrorEntry {
                    code: "CONFLICT".to_string(),
                    message,
                }],
            ),
            AppError::NotFound(message) => (
                StatusCode::NOT_FOUND,
                vec![ApiErrorEntry {
                    code: "NOT_FOUND".to_string(),
                    message,
                }],
            ),
            AppError::ForeignKey(message) => (
                StatusCode::BAD_REQUEST,
                vec![ApiErrorEntry {
                    code: "FOREIGN_KEY".to_string(),
                    message,
                }],
            ),
            AppError::Business {
                status_code,
                code,
                message,
            } => (
                StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                vec![ApiErrorEntry { code, message }],
            ),
            AppError::Custom {
                status,
                code,
                message,
            } => (
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                vec![ApiErrorEntry { code, message }],
            ),
            AppError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                vec![ApiErrorEntry {
                    code: "INTERNAL_ERROR".to_string(),
                    message: "Internal server error".to_string(),
                }],
            ),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, errors) = self.into_parts();
        let body = Json(serde_json::json!({ "errors": errors }));
        (status, body).into_response()
    }
}

/// Classifies a driver constraint code into an `AppError`, keyed on the code
/// each sqlx backend reports: sqlite extended codes (`787`/`2067`/`1555`/`1299`),
/// postgres SQLSTATE (`23503`/`23505`/`23502`), and mysql SQLSTATE `23000`.
/// MySQL collapses unique, FK, and not-null all onto `23000`, so it is split on
/// the message text (there is no code-level distinction). Uses only the generic
/// `DatabaseError` surface so this shared file compiles for any dialect selection.
fn classify_db_error(code: &str, message: &str) -> Option<AppError> {
    if code == "23000" {
        let lower = message.to_ascii_lowercase();
        if lower.contains("foreign key") {
            return Some(AppError::ForeignKey(message.to_string()));
        }
        if lower.contains("cannot be null") {
            return Some(AppError::Validation(message.to_string()));
        }
        return Some(AppError::Conflict(message.to_string()));
    }
    if code == "2067" || code == "1555" || code == "23505" {
        return Some(AppError::Conflict(message.to_string()));
    }
    if code == "787" || code == "23503" {
        return Some(AppError::ForeignKey(message.to_string()));
    }
    if code == "1299" || code == "23502" {
        return Some(AppError::Validation(message.to_string()));
    }
    if code.starts_with("SQLITE_CONSTRAINT") || code.starts_with("23") {
        return Some(AppError::Validation(message.to_string()));
    }
    None
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db_err) = &err {
            if let Some(code) = db_err.code() {
                if let Some(app) = classify_db_error(code.as_ref(), db_err.message()) {
                    return app;
                }
            }
        }
        AppError::Internal
    }
}

pub struct ErrorHandler;

impl ErrorHandler {
    pub fn new() -> Self {
        Self
    }

    pub fn envelope(status: u16, code: &str, message: &str) -> Response {
        AppError::Custom {
            status,
            code: code.to_string(),
            message: message.to_string(),
        }
        .into_response()
    }

    pub fn envelope_many(status: u16, errors: Vec<ApiErrorEntry>) -> Response {
        let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = Json(serde_json::json!({ "errors": errors }));
        (status, body).into_response()
    }
}

impl Default for ErrorHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn body_bytes(response: Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes()
            .to_vec()
    }

    #[tokio::test]
    async fn validation_error_returns_400_with_validation_error_envelope() {
        let response = AppError::Validation("bad input".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(response).await;
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "errors": [
                    { "code": "VALIDATION_ERROR", "message": "bad input" }
                ]
            })
        );
    }

    #[tokio::test]
    async fn conflict_error_returns_409_with_conflict_envelope() {
        let response = AppError::Conflict("duplicate".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = body_bytes(response).await;
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "errors": [
                    { "code": "CONFLICT", "message": "duplicate" }
                ]
            })
        );
    }

    #[tokio::test]
    async fn foreign_key_error_returns_400_with_foreign_key_code() {
        let response = AppError::ForeignKey("fk".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(response).await;
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["errors"][0]["code"].as_str().unwrap(), "FOREIGN_KEY");
    }

    #[test]
    fn classify_postgres_and_sqlite_codes() {
        assert!(matches!(
            classify_db_error("23503", "fk"),
            Some(AppError::ForeignKey(_))
        ));
        assert!(matches!(
            classify_db_error("23505", "dup"),
            Some(AppError::Conflict(_))
        ));
        assert!(matches!(
            classify_db_error("23502", "nn"),
            Some(AppError::Validation(_))
        ));
        assert!(matches!(
            classify_db_error("787", "fk"),
            Some(AppError::ForeignKey(_))
        ));
        assert!(matches!(
            classify_db_error("2067", "dup"),
            Some(AppError::Conflict(_))
        ));
        assert!(classify_db_error("08006", "connection lost").is_none());
    }

    #[test]
    fn classify_mysql_23000_splits_on_message() {
        assert!(matches!(
            classify_db_error(
                "23000",
                "Cannot add or update a child row: a foreign key constraint fails"
            ),
            Some(AppError::ForeignKey(_))
        ));
        assert!(matches!(
            classify_db_error("23000", "Column 'name' cannot be null"),
            Some(AppError::Validation(_))
        ));
        assert!(matches!(
            classify_db_error("23000", "Duplicate entry 'x' for key 'name'"),
            Some(AppError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn business_error_returns_supplied_status_and_code() {
        let response = AppError::Business {
            status_code: 401,
            code: "INVALID_TOKEN".to_string(),
            message: "token expired".to_string(),
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = body_bytes(response).await;
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["errors"][0]["code"].as_str().unwrap(), "INVALID_TOKEN");
        assert_eq!(v["errors"][0]["message"].as_str().unwrap(), "token expired");
    }

    #[tokio::test]
    async fn internal_error_returns_500_with_internal_error_envelope() {
        let response = AppError::Internal.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_bytes(response).await;
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
    async fn validation_many_returns_400_with_multiple_entries() {
        let response = AppError::ValidationMany(vec![
            ApiErrorEntry {
                code: "VALIDATION_ERROR".to_string(),
                message: "email: required".to_string(),
            },
            ApiErrorEntry {
                code: "VALIDATION_ERROR".to_string(),
                message: "name: required".to_string(),
            },
        ])
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = body_bytes(response).await;
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["errors"].as_array().unwrap().len(), 2);
        assert_eq!(
            v["errors"][1]["message"].as_str().unwrap(),
            "name: required"
        );
    }

    #[tokio::test]
    async fn error_handler_envelope_helper_produces_same_shape_as_app_error_custom() {
        let r1 = ErrorHandler::envelope(403, "FORBIDDEN", "nope");
        let r2 = AppError::Custom {
            status: 403,
            code: "FORBIDDEN".to_string(),
            message: "nope".to_string(),
        }
        .into_response();
        assert_eq!(r1.status(), r2.status());
        let b1 = body_bytes(r1).await;
        let b2 = body_bytes(r2).await;
        assert_eq!(b1, b2);
    }
}
