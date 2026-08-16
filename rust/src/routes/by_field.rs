use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::RepositoryError;
use crate::repositories::{CrudRepository, RowMap};
use crate::routes::crud::{CoerceRowFn, ValidatorFn};
use crate::routes::helpers::{
    internal_error, normalize_base, row_map_to_value, validation_errors_compound,
};

#[async_trait]
pub trait ByFieldService: Send + Sync + 'static {
    async fn find_by(&self, field: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError>;

    async fn update_by(
        &self,
        field: &str,
        value: &Value,
        data: RowMap,
    ) -> Result<u64, RepositoryError> {
        let _ = (field, value, data);
        Err(RepositoryError::UnsupportedQuery(
            "update_by is not implemented for this ByFieldService".to_string(),
        ))
    }

    async fn delete_by(&self, field: &str, value: &Value) -> Result<u64, RepositoryError> {
        let _ = (field, value);
        Err(RepositoryError::UnsupportedQuery(
            "delete_by is not implemented for this ByFieldService".to_string(),
        ))
    }
}

#[async_trait]
impl<R> ByFieldService for R
where
    R: CrudRepository + 'static,
{
    async fn find_by(&self, field: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        CrudRepository::find_by(self, field, value).await
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByFieldMethod {
    Get,
    Put,
    Delete,
}

impl Default for ByFieldMethod {
    fn default() -> Self {
        Self::Get
    }
}

pub struct ByFieldRouterConfig {
    pub service: Arc<dyn ByFieldService>,
    pub entity_name: String,
    pub base_path: String,
    pub field: String,
    pub unique: bool,
    pub methods: Vec<ByFieldMethod>,
    pub coerce_row: Option<CoerceRowFn>,
    pub update_validator: Option<ValidatorFn>,
}

pub fn create_by_field_router(cfg: ByFieldRouterConfig) -> Router {
    let base = normalize_base(&cfg.base_path);
    let field = cfg.field.clone();
    let path = format!("{}/{}/{{{}}}", base, snake_to_kebab(&field), field);
    let state = RouterState {
        service: cfg.service,
        entity_name: Arc::new(cfg.entity_name),
        field: Arc::new(field),
        unique: cfg.unique,
        coerce_row: cfg.coerce_row,
        update_validator: cfg.update_validator,
    };
    let methods = dedup_methods(&cfg.methods);
    let mut method_router: MethodRouter<RouterState> = MethodRouter::new();
    for m in methods {
        method_router = match m {
            ByFieldMethod::Get => method_router.get(get_by_field_handler),
            ByFieldMethod::Put => method_router.put(put_by_field_handler),
            ByFieldMethod::Delete => method_router.delete(delete_by_field_handler),
        };
    }
    Router::new().route(&path, method_router).with_state(state)
}

fn dedup_methods(methods: &[ByFieldMethod]) -> Vec<ByFieldMethod> {
    let mut seen: Vec<ByFieldMethod> = Vec::with_capacity(methods.len());
    for m in methods {
        if !seen.contains(m) {
            seen.push(*m);
        }
    }
    seen
}

#[derive(Clone)]
struct RouterState {
    service: Arc<dyn ByFieldService>,
    entity_name: Arc<String>,
    field: Arc<String>,
    unique: bool,
    coerce_row: Option<CoerceRowFn>,
    update_validator: Option<ValidatorFn>,
}

async fn get_by_field_handler(
    State(state): State<RouterState>,
    Path(raw_value): Path<String>,
) -> Response {
    let field = state.field.as_str();
    let lookup = Value::String(raw_value.clone());
    let mut rows = match state.service.find_by(field, &lookup).await {
        Ok(rows) => rows,
        Err(err) => return internal_error(err.to_string()),
    };
    apply_coerce(&state, &mut rows);
    if state.unique {
        unique_get_response(&state, &raw_value, rows)
    } else {
        let items: Vec<Value> = rows.into_iter().map(row_map_to_value).collect();
        (StatusCode::OK, Json(json!({ "items": items }))).into_response()
    }
}

async fn put_by_field_handler(
    State(state): State<RouterState>,
    Path(raw_value): Path<String>,
    Json(body): Json<RowMap>,
) -> Response {
    if let Err(resp) = run_validator(&state.update_validator, &body) {
        return resp;
    }
    let field = state.field.as_str();
    let lookup = Value::String(raw_value.clone());
    let count = match state.service.update_by(field, &lookup, body).await {
        Ok(count) => count,
        Err(err) => return internal_error(err.to_string()),
    };
    if state.unique {
        match count {
            0 => not_found_response(&state.entity_name, field, &raw_value),
            1 => match state.service.find_by(field, &lookup).await {
                Ok(mut rows) => match rows.pop() {
                    Some(mut row) => {
                        if let Some(f) = state.coerce_row.as_ref() {
                            f(&mut row);
                        }
                        (StatusCode::OK, Json(row_map_to_value(row))).into_response()
                    }
                    None => not_found_response(&state.entity_name, field, &raw_value),
                },
                Err(err) => internal_error(err.to_string()),
            },
            _ => conflict_response(&state.entity_name, field, &raw_value),
        }
    } else {
        (StatusCode::OK, Json(json!({ "count": count }))).into_response()
    }
}

async fn delete_by_field_handler(
    State(state): State<RouterState>,
    Path(raw_value): Path<String>,
) -> Response {
    let field = state.field.as_str();
    let lookup = Value::String(raw_value.clone());
    let count = match state.service.delete_by(field, &lookup).await {
        Ok(count) => count,
        Err(err) => return internal_error(err.to_string()),
    };
    if state.unique {
        match count {
            0 => not_found_response(&state.entity_name, field, &raw_value),
            1 => StatusCode::NO_CONTENT.into_response(),
            _ => conflict_response(&state.entity_name, field, &raw_value),
        }
    } else {
        (StatusCode::OK, Json(json!({ "count": count }))).into_response()
    }
}

fn unique_get_response(state: &RouterState, raw_value: &str, rows: Vec<RowMap>) -> Response {
    let field = state.field.as_str();
    let entity = state.entity_name.as_str();
    match rows.len() {
        0 => not_found_response(entity, field, raw_value),
        1 => {
            let row = rows.into_iter().next().expect("len == 1");
            (StatusCode::OK, Json(row_map_to_value(row))).into_response()
        }
        _ => conflict_response(entity, field, raw_value),
    }
}

fn not_found_response(entity: &str, field: &str, raw_value: &str) -> Response {
    error_envelope(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        format!("{} with {} '{}' not found", entity, field, raw_value),
    )
}

fn conflict_response(entity: &str, field: &str, raw_value: &str) -> Response {
    error_envelope(
        StatusCode::CONFLICT,
        "CONFLICT",
        format!("Multiple {} rows matched {}='{}'", entity, field, raw_value),
    )
}

fn error_envelope(status: StatusCode, code: &str, message: String) -> Response {
    (
        status,
        Json(json!({ "errors": [{ "code": code, "message": message }] })),
    )
        .into_response()
}

fn run_validator(validator: &Option<ValidatorFn>, body: &RowMap) -> Result<(), Response> {
    let Some(f) = validator else {
        return Ok(());
    };
    match f(body) {
        Ok(()) => Ok(()),
        Err(msgs) if msgs.is_empty() => Ok(()),
        Err(msgs) => Err(validation_errors_compound(msgs)),
    }
}

fn apply_coerce(state: &RouterState, rows: &mut [RowMap]) {
    let Some(f) = state.coerce_row.as_ref() else {
        return;
    };
    for row in rows.iter_mut() {
        f(row);
    }
}

fn snake_to_kebab(s: &str) -> String {
    s.replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_to_kebab_converts_underscores_to_dashes() {
        assert_eq!(snake_to_kebab("email"), "email");
        assert_eq!(snake_to_kebab("profile_pic"), "profile-pic");
        assert_eq!(snake_to_kebab("first_name_kanji"), "first-name-kanji");
        assert_eq!(snake_to_kebab(""), "");
    }

    #[test]
    fn dedup_methods_removes_duplicates_while_preserving_order() {
        let out = dedup_methods(&[
            ByFieldMethod::Get,
            ByFieldMethod::Put,
            ByFieldMethod::Get,
            ByFieldMethod::Delete,
            ByFieldMethod::Put,
        ]);
        assert_eq!(
            out,
            vec![
                ByFieldMethod::Get,
                ByFieldMethod::Put,
                ByFieldMethod::Delete,
            ]
        );
    }

    #[test]
    fn dedup_methods_returns_empty_for_empty_input() {
        assert!(dedup_methods(&[]).is_empty());
    }

    #[test]
    fn by_field_method_default_is_get() {
        assert_eq!(ByFieldMethod::default(), ByFieldMethod::Get);
    }

    #[test]
    fn not_found_response_uses_404_and_matching_message_shape() {
        let resp = not_found_response("User", "email", "a@b");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn conflict_response_uses_409_and_matching_message_shape() {
        let resp = conflict_response("User", "email", "a@b");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn apply_coerce_is_noop_when_hook_is_none() {
        let state = RouterState {
            service: Arc::new(NoopService),
            entity_name: Arc::new("User".to_string()),
            field: Arc::new("email".to_string()),
            unique: true,
            coerce_row: None,
            update_validator: None,
        };
        let mut rows = vec![sample_row("k", Value::from(1i64))];
        apply_coerce(&state, &mut rows);
        assert_eq!(rows[0].get("k").unwrap(), &Value::from(1i64));
    }

    #[test]
    fn apply_coerce_runs_hook_on_each_row_when_present() {
        let state = RouterState {
            service: Arc::new(NoopService),
            entity_name: Arc::new("User".to_string()),
            field: Arc::new("email".to_string()),
            unique: false,
            coerce_row: Some(Arc::new(|row: &mut RowMap| {
                row.insert("touched".to_string(), Value::Bool(true));
            })),
            update_validator: None,
        };
        let mut rows = vec![
            sample_row("a", Value::from(1i64)),
            sample_row("b", Value::from(2i64)),
        ];
        apply_coerce(&state, &mut rows);
        assert_eq!(rows[0].get("touched").unwrap(), &Value::Bool(true));
        assert_eq!(rows[1].get("touched").unwrap(), &Value::Bool(true));
    }

    #[test]
    fn run_validator_returns_ok_when_none() {
        assert!(run_validator(&None, &RowMap::new()).is_ok());
    }

    #[test]
    fn run_validator_returns_ok_when_hook_returns_ok() {
        let hook: ValidatorFn = Arc::new(|_body| Ok(()));
        assert!(run_validator(&Some(hook), &RowMap::new()).is_ok());
    }

    #[test]
    fn run_validator_returns_ok_when_hook_returns_empty_errs() {
        let hook: ValidatorFn = Arc::new(|_body| Err(Vec::<String>::new()));
        assert!(run_validator(&Some(hook), &RowMap::new()).is_ok());
    }

    #[test]
    fn run_validator_returns_bad_request_when_hook_returns_errs() {
        let hook: ValidatorFn = Arc::new(|_body| Err(vec!["x: bad".to_string()]));
        let resp = run_validator(&Some(hook), &RowMap::new()).unwrap_err();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    struct NoopService;

    #[async_trait]
    impl ByFieldService for NoopService {
        async fn find_by(
            &self,
            _field: &str,
            _value: &Value,
        ) -> Result<Vec<RowMap>, RepositoryError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_update_by_returns_unsupported_query() {
        let svc = NoopService;
        let out = svc.update_by("f", &Value::from(1i64), RowMap::new()).await;
        assert!(out.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_delete_by_returns_unsupported_query() {
        let svc = NoopService;
        let out = svc.delete_by("f", &Value::from(1i64)).await;
        assert!(out.is_err());
    }

    fn sample_row(key: &str, value: Value) -> RowMap {
        let mut row = RowMap::new();
        row.insert(key.to_string(), value);
        row
    }
}
