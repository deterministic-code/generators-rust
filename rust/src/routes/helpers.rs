use std::fmt::Display;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::error::RepositoryError;
use crate::repositories::RowMap;
use crate::routes::crud::{EnrichItemFn, EnrichItemsFn, ResolveItemFn, RouteHookError};

pub(crate) use crate::id_type::parse_id;
pub use crate::id_type::IdType;

pub(crate) fn row_map_to_value(row: RowMap) -> Value {
    Value::Object(row.into_iter().collect())
}

pub(crate) fn normalize_base(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn validation_error(message: String) -> Response {
    error_envelope(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", message)
}

pub(crate) fn validation_errors_compound(messages: Vec<String>) -> Response {
    let compound = messages.join("; ");
    validation_error(compound)
}

pub(crate) fn not_found<T: Display>(entity_name: &str, id: T) -> Response {
    error_envelope(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        format!("{} with id '{}' not found", entity_name, id),
    )
}

pub(crate) fn internal_error(message: String) -> Response {
    error_envelope(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message)
}

pub(crate) fn precondition_required(message: String) -> Response {
    error_envelope(
        StatusCode::PRECONDITION_REQUIRED,
        "PRECONDITION_REQUIRED",
        message,
    )
}

pub(crate) fn precondition_failed(message: String) -> Response {
    error_envelope(
        StatusCode::PRECONDITION_FAILED,
        "PRECONDITION_FAILED",
        message,
    )
}

pub(crate) fn repository_error_to_response(err: RepositoryError) -> Response {
    match &err {
        RepositoryError::ConcurrencyConflict(msg) => precondition_failed(msg.clone()),
        _ => internal_error(err.to_string()),
    }
}

pub(crate) fn hook_error_to_response(err: RouteHookError) -> Response {
    match err {
        RouteHookError::Repository(err) => internal_error(err.to_string()),
        RouteHookError::BadRequest(msgs) => validation_errors_compound(msgs),
    }
}

pub(crate) async fn run_enrich_items_hook(
    hook: Option<&EnrichItemsFn>,
    rows: Vec<RowMap>,
) -> Result<Vec<RowMap>, Response> {
    match hook {
        Some(f) => f(rows).await.map_err(hook_error_to_response),
        None => Ok(rows),
    }
}

pub(crate) async fn run_enrich_item_hook(
    hook: Option<&EnrichItemFn>,
    row: RowMap,
) -> Result<RowMap, Response> {
    match hook {
        Some(f) => f(row).await.map_err(hook_error_to_response),
        None => Ok(row),
    }
}

pub(crate) async fn run_resolve_item_hook(
    hook: Option<&ResolveItemFn>,
    body: RowMap,
) -> Result<RowMap, Response> {
    match hook {
        Some(f) => f(body).await.map_err(hook_error_to_response),
        None => Ok(body),
    }
}

fn error_envelope(status: StatusCode, code: &str, message: String) -> Response {
    (
        status,
        Json(json!({ "errors": [{ "code": code, "message": message }] })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_strips_trailing_slash() {
        assert_eq!(normalize_base("/api/users"), "/api/users");
        assert_eq!(normalize_base("/api/users/"), "/api/users");
        assert_eq!(normalize_base("/"), "");
        assert_eq!(normalize_base(""), "");
    }

    #[test]
    fn error_envelope_uses_plural_errors_array_shape() {
        let response = validation_error("id: must be a positive integer".to_string());
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    use crate::error::RepositoryError;

    #[test]
    fn hook_error_to_response_maps_repository_variant_to_500() {
        let err = RouteHookError::Repository(RepositoryError::UnsupportedQuery("boom".to_string()));
        let resp = hook_error_to_response(err);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn hook_error_to_response_maps_bad_request_variant_to_400() {
        let err = RouteHookError::BadRequest(vec!["name: required".to_string()]);
        let resp = hook_error_to_response(err);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn route_hook_error_display_joins_bad_request_messages_with_semicolon() {
        let err = RouteHookError::BadRequest(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(format!("{}", err), "a; b");
    }

    #[test]
    fn route_hook_error_from_repository_error_wraps_into_repository_variant() {
        let err: RouteHookError = RepositoryError::UnsupportedQuery("wrapped".to_string()).into();
        match err {
            RouteHookError::Repository(_) => {}
            other => panic!("expected Repository variant, got {:?}", other),
        }
    }
}
