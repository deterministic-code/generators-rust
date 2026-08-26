use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::RepositoryError;
use crate::repositories::{CrudRepository, RowMap};
use crate::routes::helpers::{
    internal_error, normalize_base, not_found, parse_id, precondition_required,
    repository_error_to_response, row_map_to_value, run_enrich_item_hook, run_enrich_items_hook,
    run_resolve_item_hook, validation_error, validation_errors_compound, IdType,
};

pub type ValidatorFn = Arc<dyn Fn(&RowMap) -> Result<(), Vec<String>> + Send + Sync + 'static>;
pub type CoerceRowFn = Arc<dyn Fn(&mut RowMap) + Send + Sync + 'static>;

#[derive(Debug)]
pub enum RouteHookError {
    Repository(RepositoryError),
    BadRequest(Vec<String>),
}

impl From<RepositoryError> for RouteHookError {
    fn from(err: RepositoryError) -> Self {
        Self::Repository(err)
    }
}

impl std::fmt::Display for RouteHookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Repository(err) => write!(f, "{}", err),
            Self::BadRequest(msgs) => write!(f, "{}", msgs.join("; ")),
        }
    }
}

pub type EnrichmentFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, RouteHookError>> + Send + 'static>>;
pub type EnrichItemsFn =
    Arc<dyn Fn(Vec<RowMap>) -> EnrichmentFuture<Vec<RowMap>> + Send + Sync + 'static>;
pub type EnrichItemFn = Arc<dyn Fn(RowMap) -> EnrichmentFuture<RowMap> + Send + Sync + 'static>;
pub type ResolveItemFn = Arc<dyn Fn(RowMap) -> EnrichmentFuture<RowMap> + Send + Sync + 'static>;

#[async_trait]
pub trait CrudService: Send + Sync + 'static {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError>;
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError>;
    async fn add(&self, body: RowMap) -> Result<RowMap, RepositoryError>;
    async fn update(
        &self,
        id: &Value,
        body: RowMap,
        expected_updated: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError>;
    async fn delete(
        &self,
        id: &Value,
        expected_updated: Option<&str>,
    ) -> Result<bool, RepositoryError>;
}

#[async_trait]
impl<R> CrudService for R
where
    R: CrudRepository + 'static,
{
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        CrudRepository::find_all(self).await
    }
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        CrudRepository::find(self, id).await
    }
    async fn add(&self, body: RowMap) -> Result<RowMap, RepositoryError> {
        CrudRepository::add(self, body).await
    }
    async fn update(
        &self,
        id: &Value,
        body: RowMap,
        expected_updated: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        CrudRepository::update_with_expected(self, id, body, expected_updated).await
    }
    async fn delete(
        &self,
        id: &Value,
        expected_updated: Option<&str>,
    ) -> Result<bool, RepositoryError> {
        CrudRepository::delete_with_expected(self, id, expected_updated).await
    }
}

pub struct CrudRouterConfig {
    pub service: Arc<dyn CrudService>,
    pub entity_name: String,
    pub base_path: String,
    pub id_type: IdType,
    pub primary_key_param: Option<String>,
    pub primary_key_params: Vec<String>,
    pub use_optimistic_concurrency: bool,
    pub create_validator: Option<ValidatorFn>,
    pub update_validator: Option<ValidatorFn>,
    pub patch_validator: Option<ValidatorFn>,
    pub coerce_row: Option<CoerceRowFn>,
    pub enrich_items: Option<EnrichItemsFn>,
    pub enrich_item: Option<EnrichItemFn>,
    pub resolve_item: Option<ResolveItemFn>,
}

pub fn create_crud_router(cfg: CrudRouterConfig) -> Router {
    let base = normalize_base(&cfg.base_path);
    let pks = if cfg.primary_key_params.is_empty() {
        vec![cfg
            .primary_key_param
            .clone()
            .unwrap_or_else(|| "id".to_string())]
    } else {
        cfg.primary_key_params.clone()
    };
    let list_path = base.clone();
    let member_path = format!(
        "{}/{}",
        base,
        pks.iter()
            .map(|pk| format!("{{{pk}}}"))
            .collect::<Vec<_>>()
            .join("/")
    );
    let state = RouterState {
        service: cfg.service,
        entity_name: Arc::new(cfg.entity_name),
        id_type: cfg.id_type,
        primary_key_param: Arc::new(pks[0].clone()),
        primary_key_params: Arc::new(pks),
        use_optimistic_concurrency: cfg.use_optimistic_concurrency,
        create_validator: cfg.create_validator,
        update_validator: cfg.update_validator,
        patch_validator: cfg.patch_validator,
        coerce_row: cfg.coerce_row,
        enrich_items: cfg.enrich_items,
        enrich_item: cfg.enrich_item,
        resolve_item: cfg.resolve_item,
    };
    Router::new()
        .route(&list_path, get(list_handler).post(create_handler))
        .route(
            &member_path,
            get(get_by_id_handler)
                .put(update_handler)
                .patch(patch_handler)
                .delete(delete_handler),
        )
        .with_state(state)
}

#[derive(Clone)]
struct RouterState {
    service: Arc<dyn CrudService>,
    entity_name: Arc<String>,
    id_type: IdType,
    primary_key_param: Arc<String>,
    primary_key_params: Arc<Vec<String>>,
    use_optimistic_concurrency: bool,
    create_validator: Option<ValidatorFn>,
    update_validator: Option<ValidatorFn>,
    patch_validator: Option<ValidatorFn>,
    coerce_row: Option<CoerceRowFn>,
    enrich_items: Option<EnrichItemsFn>,
    enrich_item: Option<EnrichItemFn>,
    resolve_item: Option<ResolveItemFn>,
}

async fn run_enrich_items(state: &RouterState, rows: Vec<RowMap>) -> Result<Vec<RowMap>, Response> {
    run_enrich_items_hook(state.enrich_items.as_ref(), rows).await
}

async fn run_enrich_item(state: &RouterState, row: RowMap) -> Result<RowMap, Response> {
    run_enrich_item_hook(state.enrich_item.as_ref(), row).await
}

async fn run_resolve_item(state: &RouterState, body: RowMap) -> Result<RowMap, Response> {
    run_resolve_item_hook(state.resolve_item.as_ref(), body).await
}

fn apply_coerce(state: &RouterState, row: &mut RowMap) {
    if let Some(f) = &state.coerce_row {
        f(row);
    }
}

fn coerce_and_wrap(state: &RouterState, mut row: RowMap) -> Value {
    apply_coerce(state, &mut row);
    row_map_to_value(row)
}

fn extract_id(state: &RouterState, raw: &str) -> Result<Value, Response> {
    parse_id(state.id_type, raw).map_err(validation_error)
}

fn extract_identity(
    state: &RouterState,
    params: &std::collections::HashMap<String, String>,
) -> Result<Value, Response> {
    if state.primary_key_params.len() <= 1 {
        let name = state.primary_key_param.as_str();
        let raw = params
            .get(name)
            .or_else(|| params.values().next())
            .map(String::as_str)
            .unwrap_or("");
        return extract_id(state, raw);
    }
    let mut obj = serde_json::Map::new();
    for name in state.primary_key_params.iter() {
        let raw = params.get(name).map(String::as_str).unwrap_or("");
        let value = parse_id(state.id_type, raw).map_err(validation_error)?;
        obj.insert(name.clone(), value);
    }
    Ok(Value::Object(obj))
}

fn require_if_match(
    state: &RouterState,
    method: &str,
    headers: &HeaderMap,
) -> Result<Option<String>, Response> {
    if !state.use_optimistic_concurrency {
        return Ok(None);
    }
    match headers.get("if-match").and_then(|v| v.to_str().ok()) {
        Some(v) if !v.trim().is_empty() => Ok(Some(v.trim().trim_matches('"').to_string())),
        _ => Err(precondition_required(format!(
            "If-Match header required for {} on {}",
            method, state.entity_name
        ))),
    }
}

fn strip_pk_from_body(state: &RouterState, body: &mut RowMap) {
    for name in state.primary_key_params.iter() {
        body.remove(name);
    }
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

fn resolve_mutation_validator<'a>(state: &'a RouterState, method: &str) -> &'a Option<ValidatorFn> {
    if method == "PATCH" && state.patch_validator.is_some() {
        &state.patch_validator
    } else {
        &state.update_validator
    }
}

async fn list_handler(State(state): State<RouterState>) -> impl IntoResponse {
    let rows = match state.service.find_all().await {
        Ok(rows) => rows,
        Err(err) => return internal_error(err.to_string()),
    };
    let rows = match run_enrich_items(&state, rows).await {
        Ok(rows) => rows,
        Err(resp) => return resp,
    };
    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| coerce_and_wrap(&state, row))
        .collect();
    (StatusCode::OK, Json(json!({ "items": items }))).into_response()
}

async fn get_by_id_handler(
    State(state): State<RouterState>,
    Path(params): Path<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let id_value = match extract_identity(&state, &params) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let row = match state.service.find(&id_value).await {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.entity_name, display_id(&id_value)),
        Err(err) => return internal_error(err.to_string()),
    };
    let row = match run_enrich_item(&state, row).await {
        Ok(row) => row,
        Err(resp) => return resp,
    };
    (StatusCode::OK, Json(coerce_and_wrap(&state, row))).into_response()
}

async fn create_handler(
    State(state): State<RouterState>,
    Json(body): Json<RowMap>,
) -> impl IntoResponse {
    if let Err(resp) = run_validator(&state.create_validator, &body) {
        return resp;
    }
    let body = match run_resolve_item(&state, body).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let row = match state.service.add(body).await {
        Ok(row) => row,
        Err(err) => return internal_error(err.to_string()),
    };
    let row = match run_enrich_item(&state, row).await {
        Ok(row) => row,
        Err(resp) => return resp,
    };
    (StatusCode::CREATED, Json(coerce_and_wrap(&state, row))).into_response()
}

async fn mutation_handler(
    state: RouterState,
    method: &str,
    params: std::collections::HashMap<String, String>,
    headers: HeaderMap,
    mut body: RowMap,
) -> Response {
    let expected_updated = match require_if_match(&state, method, &headers) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Err(resp) = run_validator(resolve_mutation_validator(&state, method), &body) {
        return resp;
    }
    let id_value = match extract_identity(&state, &params) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    strip_pk_from_body(&state, &mut body);
    let body = match run_resolve_item(&state, body).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let row = match state
        .service
        .update(&id_value, body, expected_updated.as_deref())
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.entity_name, display_id(&id_value)),
        Err(err) => return repository_error_to_response(err),
    };
    let row = match run_enrich_item(&state, row).await {
        Ok(row) => row,
        Err(resp) => return resp,
    };
    (StatusCode::OK, Json(coerce_and_wrap(&state, row))).into_response()
}

async fn update_handler(
    State(state): State<RouterState>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<RowMap>,
) -> Response {
    mutation_handler(state, "PUT", params, headers, body).await
}

async fn patch_handler(
    State(state): State<RouterState>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<RowMap>,
) -> Response {
    mutation_handler(state, "PATCH", params, headers, body).await
}

async fn delete_handler(
    State(state): State<RouterState>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let expected_updated = match require_if_match(&state, "DELETE", &headers) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let id_value = match extract_identity(&state, &params) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match state
        .service
        .delete(&id_value, expected_updated.as_deref())
        .await
    {
        Ok(true) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
        Ok(false) => not_found(&state.entity_name, display_id(&id_value)),
        Err(err) => repository_error_to_response(err),
    }
}

fn display_id(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn display_id_renders_string_without_quotes() {
        assert_eq!(display_id(&Value::String("abc".to_string())), "abc");
    }

    #[test]
    fn display_id_renders_number_bare() {
        assert_eq!(display_id(&Value::from(42i64)), "42");
    }

    #[test]
    fn display_id_renders_other_value_types_via_to_string() {
        assert_eq!(display_id(&Value::Bool(true)), "true");
        assert_eq!(display_id(&Value::Null), "null");
    }

    #[test]
    fn default_id_type_is_integer() {
        assert_eq!(IdType::default(), IdType::Integer);
    }

    fn state_with(occ: bool) -> RouterState {
        RouterState {
            service: Arc::new(NoopCrudService),
            entity_name: Arc::new("User".to_string()),
            id_type: IdType::Integer,
            primary_key_param: Arc::new("id".to_string()),
            primary_key_params: Arc::new(vec!["id".to_string()]),
            use_optimistic_concurrency: occ,
            create_validator: None,
            update_validator: None,
            patch_validator: None,
            coerce_row: None,
            enrich_items: None,
            enrich_item: None,
            resolve_item: None,
        }
    }

    struct NoopCrudService;

    #[async_trait]
    impl CrudService for NoopCrudService {
        async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
            Ok(Vec::new())
        }
        async fn find(&self, _id: &Value) -> Result<Option<RowMap>, RepositoryError> {
            Ok(None)
        }
        async fn add(&self, body: RowMap) -> Result<RowMap, RepositoryError> {
            Ok(body)
        }
        async fn update(
            &self,
            _id: &Value,
            body: RowMap,
            _expected_updated: Option<&str>,
        ) -> Result<Option<RowMap>, RepositoryError> {
            Ok(Some(body))
        }
        async fn delete(
            &self,
            _id: &Value,
            _expected_updated: Option<&str>,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[test]
    fn require_if_match_returns_none_when_occ_disabled_regardless_of_header() {
        let state = state_with(false);
        let mut headers = HeaderMap::new();
        headers.insert("if-match", HeaderValue::from_static("anything"));
        assert_eq!(require_if_match(&state, "PUT", &headers).unwrap(), None);
        let empty = HeaderMap::new();
        assert_eq!(require_if_match(&state, "PUT", &empty).unwrap(), None);
    }

    #[test]
    fn require_if_match_returns_err_when_occ_enabled_and_header_missing() {
        let state = state_with(true);
        let empty = HeaderMap::new();
        let err = require_if_match(&state, "PUT", &empty).unwrap_err();
        assert_eq!(err.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[test]
    fn require_if_match_returns_err_when_occ_enabled_and_header_empty() {
        let state = state_with(true);
        let mut headers = HeaderMap::new();
        headers.insert("if-match", HeaderValue::from_static("   "));
        let err = require_if_match(&state, "DELETE", &headers).unwrap_err();
        assert_eq!(err.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[test]
    fn require_if_match_returns_extracted_value_when_present() {
        let state = state_with(true);
        let mut headers = HeaderMap::new();
        headers.insert("if-match", HeaderValue::from_static("token-1"));
        assert_eq!(
            require_if_match(&state, "PUT", &headers).unwrap(),
            Some("token-1".to_string()),
        );
    }

    #[test]
    fn require_if_match_strips_surrounding_double_quotes_etag_style() {
        let state = state_with(true);
        let mut headers = HeaderMap::new();
        headers.insert("if-match", HeaderValue::from_static("\"quoted-token\""));
        assert_eq!(
            require_if_match(&state, "PATCH", &headers).unwrap(),
            Some("quoted-token".to_string()),
        );
    }

    #[test]
    fn run_validator_returns_ok_when_none() {
        assert!(run_validator(&None, &RowMap::new()).is_ok());
    }

    #[test]
    fn run_validator_returns_ok_when_hook_returns_empty_errs() {
        let hook: ValidatorFn = Arc::new(|_body| Err(Vec::<String>::new()));
        assert!(run_validator(&Some(hook), &RowMap::new()).is_ok());
    }

    #[test]
    fn run_validator_returns_ok_when_hook_returns_ok() {
        let hook: ValidatorFn = Arc::new(|_body| Ok(()));
        assert!(run_validator(&Some(hook), &RowMap::new()).is_ok());
    }

    #[test]
    fn run_validator_returns_bad_request_response_when_hook_returns_errs() {
        let hook: ValidatorFn =
            Arc::new(|_body| Err(vec!["a: bad".to_string(), "b: worse".to_string()]));
        let resp = run_validator(&Some(hook), &RowMap::new()).unwrap_err();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn apply_coerce_is_noop_when_hook_is_none() {
        let state = state_with(false);
        let mut row = RowMap::new();
        row.insert("v".to_string(), Value::from(1i64));
        apply_coerce(&state, &mut row);
        assert_eq!(row.get("v").unwrap(), &Value::from(1i64));
    }

    #[test]
    fn apply_coerce_runs_hook_when_present() {
        let mut state = state_with(false);
        state.coerce_row = Some(Arc::new(|row: &mut RowMap| {
            row.insert("touched".to_string(), Value::Bool(true));
        }));
        let mut row = RowMap::new();
        apply_coerce(&state, &mut row);
        assert_eq!(row.get("touched").unwrap(), &Value::Bool(true));
    }

    #[test]
    fn strip_pk_from_body_removes_the_configured_column() {
        let state = state_with(false);
        let mut body = RowMap::new();
        body.insert("id".to_string(), Value::from(1i64));
        body.insert("name".to_string(), Value::String("x".to_string()));
        strip_pk_from_body(&state, &mut body);
        assert!(body.get("id").is_none());
        assert!(body.get("name").is_some());
    }

    fn tagged_validator(tag: &'static str) -> ValidatorFn {
        Arc::new(move |_body: &RowMap| Err(vec![tag.to_string()]))
    }

    fn run_and_message(v: &Option<ValidatorFn>) -> Option<String> {
        let body = RowMap::new();
        v.as_ref()
            .and_then(|f| f(&body).err().and_then(|msgs| msgs.first().cloned()))
    }

    #[test]
    fn resolve_mutation_validator_returns_update_for_put_regardless_of_patch_field() {
        let mut state = state_with(false);
        state.update_validator = Some(tagged_validator("update"));
        state.patch_validator = Some(tagged_validator("patch"));
        let resolved = resolve_mutation_validator(&state, "PUT");
        assert_eq!(run_and_message(resolved).as_deref(), Some("update"));
    }

    #[test]
    fn resolve_mutation_validator_returns_patch_for_patch_when_set() {
        let mut state = state_with(false);
        state.update_validator = Some(tagged_validator("update"));
        state.patch_validator = Some(tagged_validator("patch"));
        let resolved = resolve_mutation_validator(&state, "PATCH");
        assert_eq!(run_and_message(resolved).as_deref(), Some("patch"));
    }

    #[test]
    fn resolve_mutation_validator_falls_back_to_update_for_patch_when_patch_is_none() {
        let mut state = state_with(false);
        state.update_validator = Some(tagged_validator("update"));
        state.patch_validator = None;
        let resolved = resolve_mutation_validator(&state, "PATCH");
        assert_eq!(run_and_message(resolved).as_deref(), Some("update"));
    }

    #[test]
    fn resolve_mutation_validator_returns_none_when_both_absent_for_patch() {
        let state = state_with(false);
        let resolved = resolve_mutation_validator(&state, "PATCH");
        assert!(resolved.is_none());
    }

    #[test]
    fn resolve_mutation_validator_returns_none_when_both_absent_for_put() {
        let state = state_with(false);
        let resolved = resolve_mutation_validator(&state, "PUT");
        assert!(resolved.is_none());
    }
}
