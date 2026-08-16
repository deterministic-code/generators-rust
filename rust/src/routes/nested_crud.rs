use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::repositories::RowMap;
use crate::routes::crud::{
    CoerceRowFn, CrudService, EnrichItemFn, EnrichItemsFn, ResolveItemFn, ValidatorFn,
};
use crate::routes::helpers::{
    internal_error, normalize_base, not_found, parse_id, precondition_required, row_map_to_value,
    run_enrich_item_hook, run_enrich_items_hook, run_resolve_item_hook, validation_error,
    validation_errors_compound, IdType,
};

pub struct NestedCrudRouterConfig {
    pub service: Arc<dyn CrudService>,
    pub entity_name: String,
    pub base_path: String,
    pub parent_param_name: String,
    pub parent_fk_field: String,
    pub parent_id_type: IdType,
    pub child_id_type: IdType,
    pub child_primary_key_param: Option<String>,
    pub use_optimistic_concurrency: bool,
    pub create_validator: Option<ValidatorFn>,
    pub update_validator: Option<ValidatorFn>,
    pub patch_validator: Option<ValidatorFn>,
    pub coerce_row: Option<CoerceRowFn>,
    pub enrich_items: Option<EnrichItemsFn>,
    pub enrich_item: Option<EnrichItemFn>,
    pub resolve_item: Option<ResolveItemFn>,
}

pub fn create_nested_crud_router(cfg: NestedCrudRouterConfig) -> Router {
    let base = normalize_base(&cfg.base_path);
    let child_pk = cfg
        .child_primary_key_param
        .clone()
        .unwrap_or_else(|| "id".to_string());
    let list_path = base.clone();
    let member_path = format!("{}/{{{}}}", base, child_pk);
    let state = RouterState {
        service: cfg.service,
        entity_name: Arc::new(cfg.entity_name),
        parent_param_name: Arc::new(cfg.parent_param_name),
        parent_fk_field: Arc::new(cfg.parent_fk_field),
        parent_id_type: cfg.parent_id_type,
        child_id_type: cfg.child_id_type,
        child_primary_key_param: Arc::new(child_pk),
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
    parent_param_name: Arc<String>,
    parent_fk_field: Arc<String>,
    parent_id_type: IdType,
    child_id_type: IdType,
    child_primary_key_param: Arc<String>,
    use_optimistic_concurrency: bool,
    create_validator: Option<ValidatorFn>,
    update_validator: Option<ValidatorFn>,
    patch_validator: Option<ValidatorFn>,
    coerce_row: Option<CoerceRowFn>,
    enrich_items: Option<EnrichItemsFn>,
    enrich_item: Option<EnrichItemFn>,
    resolve_item: Option<ResolveItemFn>,
}

fn filter_by_parent(rows: Vec<RowMap>, fk_field: &str, parent_id: &Value) -> Vec<RowMap> {
    rows.into_iter()
        .filter(|r| r.get(fk_field) == Some(parent_id))
        .collect()
}

fn row_belongs_to_parent(row: &RowMap, fk_field: &str, parent_id: &Value) -> bool {
    row.get(fk_field) == Some(parent_id)
}

fn pin_parent_fk(body: &mut RowMap, fk_field: &str, parent_id: &Value) {
    body.insert(fk_field.to_string(), parent_id.clone());
}

fn display_id(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn parse_parent_id(state: &RouterState, raw: &str) -> Result<Value, Response> {
    parse_id(state.parent_id_type, raw).map_err(|_| {
        validation_error(match state.parent_id_type {
            IdType::Integer => format!("{}: must be a positive integer", state.parent_param_name),
            IdType::String => format!("{}: must be a non-empty string", state.parent_param_name),
            IdType::Uuid => format!("{}: must be a valid uuid", state.parent_param_name),
        })
    })
}

fn parse_child_id(state: &RouterState, raw: &str) -> Result<Value, Response> {
    parse_id(state.child_id_type, raw).map_err(validation_error)
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

fn strip_child_pk(state: &RouterState, body: &mut RowMap) {
    body.remove(state.child_primary_key_param.as_str());
}

async fn list_handler(
    State(state): State<RouterState>,
    Path(parent_raw): Path<String>,
) -> Response {
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let all = match state.service.find_all().await {
        Ok(rows) => rows,
        Err(err) => return internal_error(err.to_string()),
    };
    let filtered = filter_by_parent(all, state.parent_fk_field.as_str(), &parent_id);
    let filtered = match run_enrich_items(&state, filtered).await {
        Ok(rows) => rows,
        Err(resp) => return resp,
    };
    let items: Vec<Value> = filtered
        .into_iter()
        .map(|row| coerce_and_wrap(&state, row))
        .collect();
    (StatusCode::OK, Json(json!({ "items": items }))).into_response()
}

async fn get_by_id_handler(
    State(state): State<RouterState>,
    Path((parent_raw, child_raw)): Path<(String, String)>,
) -> Response {
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match parse_child_id(&state, &child_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let row = match state.service.find(&child_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.entity_name, display_id(&child_id)),
        Err(err) => return internal_error(err.to_string()),
    };
    if !row_belongs_to_parent(&row, state.parent_fk_field.as_str(), &parent_id) {
        return not_found(&state.entity_name, display_id(&child_id));
    }
    let row = match run_enrich_item(&state, row).await {
        Ok(row) => row,
        Err(resp) => return resp,
    };
    (StatusCode::OK, Json(coerce_and_wrap(&state, row))).into_response()
}

async fn create_handler(
    State(state): State<RouterState>,
    Path(parent_raw): Path<String>,
    Json(mut body): Json<RowMap>,
) -> Response {
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    pin_parent_fk(&mut body, state.parent_fk_field.as_str(), &parent_id);
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
    parent_raw: String,
    child_raw: String,
    headers: HeaderMap,
    mut body: RowMap,
) -> Response {
    let expected_updated = match require_if_match(&state, method, &headers) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match parse_child_id(&state, &child_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let existing = match state.service.find(&child_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.entity_name, display_id(&child_id)),
        Err(err) => return internal_error(err.to_string()),
    };
    if !row_belongs_to_parent(&existing, state.parent_fk_field.as_str(), &parent_id) {
        return not_found(&state.entity_name, display_id(&child_id));
    }
    if let Err(resp) = run_validator(resolve_mutation_validator(&state, method), &body) {
        return resp;
    }
    strip_child_pk(&state, &mut body);
    pin_parent_fk(&mut body, state.parent_fk_field.as_str(), &parent_id);
    let body = match run_resolve_item(&state, body).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let row = match state
        .service
        .update(&child_id, body, expected_updated.as_deref())
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.entity_name, display_id(&child_id)),
        Err(err) => return internal_error(err.to_string()),
    };
    let row = match run_enrich_item(&state, row).await {
        Ok(row) => row,
        Err(resp) => return resp,
    };
    (StatusCode::OK, Json(coerce_and_wrap(&state, row))).into_response()
}

async fn update_handler(
    State(state): State<RouterState>,
    Path((parent_raw, child_raw)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<RowMap>,
) -> Response {
    mutation_handler(state, "PUT", parent_raw, child_raw, headers, body).await
}

async fn patch_handler(
    State(state): State<RouterState>,
    Path((parent_raw, child_raw)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<RowMap>,
) -> Response {
    mutation_handler(state, "PATCH", parent_raw, child_raw, headers, body).await
}

async fn delete_handler(
    State(state): State<RouterState>,
    Path((parent_raw, child_raw)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let expected_updated = match require_if_match(&state, "DELETE", &headers) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match parse_child_id(&state, &child_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let existing = match state.service.find(&child_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.entity_name, display_id(&child_id)),
        Err(err) => return internal_error(err.to_string()),
    };
    if !row_belongs_to_parent(&existing, state.parent_fk_field.as_str(), &parent_id) {
        return not_found(&state.entity_name, display_id(&child_id));
    }
    match state
        .service
        .delete(&child_id, expected_updated.as_deref())
        .await
    {
        Ok(true) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
        Ok(false) => not_found(&state.entity_name, display_id(&child_id)),
        Err(err) => internal_error(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_if_match_returns_none_when_occ_disabled_regardless_of_header() {
        let state = state_with(false);
        let mut headers = HeaderMap::new();
        headers.insert("if-match", "anything".parse().unwrap());
        assert_eq!(require_if_match(&state, "PUT", &headers).unwrap(), None);
        assert_eq!(
            require_if_match(&state, "PUT", &HeaderMap::new()).unwrap(),
            None
        );
    }

    #[test]
    fn require_if_match_returns_428_when_occ_enabled_and_header_missing() {
        let state = state_with(true);
        let err = require_if_match(&state, "PUT", &HeaderMap::new()).unwrap_err();
        assert_eq!(err.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[test]
    fn require_if_match_returns_428_when_occ_enabled_and_header_empty() {
        let state = state_with(true);
        let mut headers = HeaderMap::new();
        headers.insert("if-match", "   ".parse().unwrap());
        let err = require_if_match(&state, "DELETE", &headers).unwrap_err();
        assert_eq!(err.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[test]
    fn require_if_match_strips_quotes_and_returns_token_when_present() {
        let state = state_with(true);
        let mut headers = HeaderMap::new();
        headers.insert("if-match", "\"token-1\"".parse().unwrap());
        assert_eq!(
            require_if_match(&state, "PATCH", &headers).unwrap(),
            Some("token-1".to_string())
        );
    }

    fn row_of(fk: i64) -> RowMap {
        let mut r = RowMap::new();
        r.insert("user_id".to_string(), Value::from(fk));
        r.insert("id".to_string(), Value::from(fk + 100));
        r
    }

    #[test]
    fn filter_by_parent_keeps_rows_whose_fk_matches_and_drops_others() {
        let rows = vec![row_of(1), row_of(2), row_of(1)];
        let out = filter_by_parent(rows, "user_id", &Value::from(1i64));
        assert_eq!(out.len(), 2);
        for row in &out {
            assert_eq!(row.get("user_id").unwrap(), &Value::from(1i64));
        }
    }

    #[test]
    fn filter_by_parent_returns_empty_when_no_rows_match() {
        let rows = vec![row_of(1), row_of(2)];
        let out = filter_by_parent(rows, "user_id", &Value::from(99i64));
        assert!(out.is_empty());
    }

    #[test]
    fn filter_by_parent_treats_missing_fk_field_as_no_match() {
        let mut row = RowMap::new();
        row.insert("id".to_string(), Value::from(1i64));
        let out = filter_by_parent(vec![row], "user_id", &Value::from(1i64));
        assert!(out.is_empty());
    }

    #[test]
    fn row_belongs_to_parent_true_when_fk_matches() {
        let row = row_of(3);
        assert!(row_belongs_to_parent(&row, "user_id", &Value::from(3i64)));
    }

    #[test]
    fn row_belongs_to_parent_false_when_fk_mismatch() {
        let row = row_of(3);
        assert!(!row_belongs_to_parent(&row, "user_id", &Value::from(4i64)));
    }

    #[test]
    fn row_belongs_to_parent_false_when_fk_field_missing() {
        let mut row = RowMap::new();
        row.insert("id".to_string(), Value::from(1i64));
        assert!(!row_belongs_to_parent(&row, "user_id", &Value::from(1i64)));
    }

    #[test]
    fn pin_parent_fk_inserts_fk_field_when_absent() {
        let mut body = RowMap::new();
        pin_parent_fk(&mut body, "user_id", &Value::from(5i64));
        assert_eq!(body.get("user_id").unwrap(), &Value::from(5i64));
    }

    #[test]
    fn pin_parent_fk_overwrites_client_supplied_fk_value() {
        let mut body = RowMap::new();
        body.insert("user_id".to_string(), Value::from(999i64));
        pin_parent_fk(&mut body, "user_id", &Value::from(5i64));
        assert_eq!(body.get("user_id").unwrap(), &Value::from(5i64));
    }

    #[test]
    fn display_id_renders_string_without_quotes_and_number_bare() {
        assert_eq!(display_id(&Value::String("k".to_string())), "k");
        assert_eq!(display_id(&Value::from(7i64)), "7");
        assert_eq!(display_id(&Value::Bool(false)), "false");
    }

    use crate::error::RepositoryError;
    use async_trait::async_trait;

    struct NoopChildService;

    #[async_trait]
    impl CrudService for NoopChildService {
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
            _e: Option<&str>,
        ) -> Result<Option<RowMap>, RepositoryError> {
            Ok(Some(body))
        }
        async fn delete(&self, _id: &Value, _e: Option<&str>) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    fn state_with(occ: bool) -> RouterState {
        RouterState {
            service: Arc::new(NoopChildService),
            entity_name: Arc::new("Comment".to_string()),
            parent_param_name: Arc::new("user_id".to_string()),
            parent_fk_field: Arc::new("user_id".to_string()),
            parent_id_type: IdType::Integer,
            child_id_type: IdType::Integer,
            child_primary_key_param: Arc::new("id".to_string()),
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

    fn tagged(tag: &'static str) -> ValidatorFn {
        Arc::new(move |_body: &RowMap| Err(vec![tag.to_string()]))
    }

    fn first_msg(v: &Option<ValidatorFn>) -> Option<String> {
        let body = RowMap::new();
        v.as_ref()
            .and_then(|f| f(&body).err().and_then(|msgs| msgs.first().cloned()))
    }

    #[test]
    fn resolve_mutation_validator_uses_update_for_put_regardless_of_patch_field() {
        let mut state = state_with(false);
        state.update_validator = Some(tagged("update"));
        state.patch_validator = Some(tagged("patch"));
        assert_eq!(
            first_msg(resolve_mutation_validator(&state, "PUT")).as_deref(),
            Some("update"),
        );
    }

    #[test]
    fn resolve_mutation_validator_uses_patch_for_patch_when_set() {
        let mut state = state_with(false);
        state.update_validator = Some(tagged("update"));
        state.patch_validator = Some(tagged("patch"));
        assert_eq!(
            first_msg(resolve_mutation_validator(&state, "PATCH")).as_deref(),
            Some("patch"),
        );
    }

    #[test]
    fn resolve_mutation_validator_falls_back_to_update_for_patch_when_patch_is_none() {
        let mut state = state_with(false);
        state.update_validator = Some(tagged("update"));
        assert_eq!(
            first_msg(resolve_mutation_validator(&state, "PATCH")).as_deref(),
            Some("update"),
        );
    }

    #[test]
    fn resolve_mutation_validator_returns_none_when_both_absent() {
        let state = state_with(false);
        assert!(resolve_mutation_validator(&state, "PUT").is_none());
        assert!(resolve_mutation_validator(&state, "PATCH").is_none());
    }
}
