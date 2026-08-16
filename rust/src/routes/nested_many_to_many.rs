use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::repositories::RowMap;
use crate::routes::crud::{CoerceRowFn, CrudService, ValidatorFn};
use crate::routes::helpers::{
    internal_error, normalize_base, not_found, parse_id, row_map_to_value, validation_error,
    validation_errors_compound, IdType,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkVerb {
    Put,
    Post,
}

impl Default for LinkVerb {
    fn default() -> Self {
        LinkVerb::Put
    }
}

pub struct NestedManyToManyRouterConfig {
    pub junction_service: Arc<dyn CrudService>,
    pub child_service: Option<Arc<dyn CrudService>>,
    pub parent_entity_name: String,
    pub child_entity_name: String,
    pub base_path: String,
    pub parent_param_name: String,
    pub child_param_name: String,
    pub parent_fk_field: String,
    pub child_fk_field: String,
    pub parent_id_type: IdType,
    pub child_id_type: IdType,
    pub create_validator: Option<ValidatorFn>,
    pub child_create_validator: Option<ValidatorFn>,
    pub patch_child_validator: Option<ValidatorFn>,
    pub coerce_row: Option<CoerceRowFn>,
    pub link_verb: LinkVerb,
    pub child_parent_fk_field: Option<String>,
}

pub fn create_nested_many_to_many_router(cfg: NestedManyToManyRouterConfig) -> Router {
    let base = normalize_base(&cfg.base_path);
    let list_path = base.clone();
    let member_path = format!("{}/{{{}}}", base, cfg.child_param_name);
    let state = RouterState {
        service: cfg.junction_service,
        child_service: cfg.child_service,
        parent_entity_name: Arc::new(cfg.parent_entity_name),
        child_entity_name: Arc::new(cfg.child_entity_name),
        parent_param_name: Arc::new(cfg.parent_param_name),
        child_param_name: Arc::new(cfg.child_param_name),
        parent_fk_field: Arc::new(cfg.parent_fk_field),
        child_fk_field: Arc::new(cfg.child_fk_field),
        parent_id_type: cfg.parent_id_type,
        child_id_type: cfg.child_id_type,
        create_validator: cfg.create_validator,
        child_create_validator: cfg.child_create_validator,
        patch_child_validator: cfg.patch_child_validator,
        coerce_row: cfg.coerce_row,
        link_verb: cfg.link_verb,
        child_parent_fk_field: cfg.child_parent_fk_field.map(Arc::new),
    };
    let member_methods = build_member_methods(&state);
    Router::new()
        .route(&list_path, get(list_handler).post(create_handler))
        .route(&member_path, member_methods)
        .with_state(state)
}

fn build_member_methods(state: &RouterState) -> MethodRouter<RouterState> {
    let mut methods = axum::routing::delete(delete_handler);
    if state.child_service.is_some() {
        methods = methods.get(get_child_handler);
        methods = match state.link_verb {
            LinkVerb::Put => methods.put(link_handler),
            LinkVerb::Post => methods.post(link_handler),
        };
        if state.patch_child_validator.is_some() {
            methods = methods.patch(patch_child_handler);
        }
    }
    methods
}

#[derive(Clone)]
struct RouterState {
    service: Arc<dyn CrudService>,
    child_service: Option<Arc<dyn CrudService>>,
    parent_entity_name: Arc<String>,
    child_entity_name: Arc<String>,
    parent_param_name: Arc<String>,
    child_param_name: Arc<String>,
    parent_fk_field: Arc<String>,
    child_fk_field: Arc<String>,
    parent_id_type: IdType,
    child_id_type: IdType,
    create_validator: Option<ValidatorFn>,
    child_create_validator: Option<ValidatorFn>,
    patch_child_validator: Option<ValidatorFn>,
    coerce_row: Option<CoerceRowFn>,
    link_verb: LinkVerb,
    child_parent_fk_field: Option<Arc<String>>,
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
    parse_id(state.child_id_type, raw).map_err(|_| {
        validation_error(match state.child_id_type {
            IdType::Integer => format!("{}: must be a positive integer", state.child_param_name),
            IdType::String => format!("{}: must be a non-empty string", state.child_param_name),
            IdType::Uuid => format!("{}: must be a valid uuid", state.child_param_name),
        })
    })
}

fn filter_by_parent_fk(rows: Vec<RowMap>, fk_field: &str, parent_id: &Value) -> Vec<RowMap> {
    rows.into_iter()
        .filter(|r| r.get(fk_field) == Some(parent_id))
        .collect()
}

fn find_junction(
    rows: &[RowMap],
    parent_fk_field: &str,
    parent_id: &Value,
    child_fk_field: &str,
    child_id: &Value,
) -> Option<RowMap> {
    rows.iter()
        .find(|r| {
            r.get(parent_fk_field) == Some(parent_id) && r.get(child_fk_field) == Some(child_id)
        })
        .cloned()
}

fn display_id(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn junction_row_id(row: &RowMap) -> Option<Value> {
    row.get("id").cloned()
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

fn apply_coerce(state: &RouterState, row: &mut RowMap) {
    if let Some(f) = &state.coerce_row {
        f(row);
    }
}

fn coerce_and_wrap(state: &RouterState, mut row: RowMap) -> Value {
    apply_coerce(state, &mut row);
    row_map_to_value(row)
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
    let filtered = filter_by_parent_fk(all, state.parent_fk_field.as_str(), &parent_id);
    let items: Vec<Value> = filtered
        .into_iter()
        .map(|row| coerce_and_wrap(&state, row))
        .collect();
    (StatusCode::OK, Json(json!({ "items": items }))).into_response()
}

async fn create_handler(
    State(state): State<RouterState>,
    Path(parent_raw): Path<String>,
    Json(body): Json<RowMap>,
) -> Response {
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if is_create_and_link_mode(&state) {
        return create_and_link_child(state, parent_id, body).await;
    }
    let mut junction_body = body;
    junction_body.insert(state.parent_fk_field.to_string(), parent_id);
    if let Err(resp) = run_validator(&state.create_validator, &junction_body) {
        return resp;
    }
    match state.service.add(junction_body).await {
        Ok(row) => (StatusCode::CREATED, Json(coerce_and_wrap(&state, row))).into_response(),
        Err(err) => internal_error(err.to_string()),
    }
}

fn is_create_and_link_mode(state: &RouterState) -> bool {
    state.child_service.is_some() && state.child_create_validator.is_some()
}

async fn create_and_link_child(
    state: RouterState,
    parent_id: Value,
    mut child_body: RowMap,
) -> Response {
    let Some(child_service) = state.child_service.clone() else {
        return internal_error("create_and_link_child called without child_service".to_string());
    };
    if let Err(resp) = run_validator(&state.child_create_validator, &child_body) {
        return resp;
    }
    if let Some(fk_field) = state.child_parent_fk_field.as_ref() {
        child_body.insert(fk_field.as_str().to_string(), parent_id.clone());
    }
    let created_child = match child_service.add(child_body).await {
        Ok(row) => row,
        Err(err) => return internal_error(err.to_string()),
    };
    let Some(child_id) = created_child.get("id").cloned() else {
        return internal_error(format!(
            "created {} row missing 'id' column — cannot link junction",
            state.child_entity_name,
        ));
    };
    let mut junction_body = RowMap::new();
    junction_body.insert(state.parent_fk_field.to_string(), parent_id);
    junction_body.insert(state.child_fk_field.to_string(), child_id);
    if let Err(err) = state.service.add(junction_body).await {
        return internal_error(err.to_string());
    }
    (
        StatusCode::CREATED,
        Json(coerce_and_wrap(&state, created_child)),
    )
        .into_response()
}

async fn delete_handler(
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
    let all = match state.service.find_all().await {
        Ok(rows) => rows,
        Err(err) => return internal_error(err.to_string()),
    };
    let Some(junction) = find_junction(
        &all,
        state.parent_fk_field.as_str(),
        &parent_id,
        state.child_fk_field.as_str(),
        &child_id,
    ) else {
        return not_found(&state.child_entity_name, display_id(&child_id));
    };
    let Some(junction_id) = junction_row_id(&junction) else {
        return internal_error(format!(
            "junction row for {} '{}' missing 'id' column",
            state.parent_entity_name,
            display_id(&parent_id),
        ));
    };
    match state.service.delete(&junction_id, None).await {
        Ok(true) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
        Ok(false) => not_found(&state.child_entity_name, display_id(&child_id)),
        Err(err) => internal_error(err.to_string()),
    }
}

async fn get_child_handler(
    State(state): State<RouterState>,
    Path((parent_raw, child_raw)): Path<(String, String)>,
) -> Response {
    let Some(child_service) = state.child_service.clone() else {
        return internal_error("get_child_handler mounted without child_service".to_string());
    };
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match parse_child_id(&state, &child_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let all = match state.service.find_all().await {
        Ok(rows) => rows,
        Err(err) => return internal_error(err.to_string()),
    };
    let junction = find_junction(
        &all,
        state.parent_fk_field.as_str(),
        &parent_id,
        state.child_fk_field.as_str(),
        &child_id,
    );
    if junction.is_none() {
        return not_found(&state.child_entity_name, display_id(&child_id));
    }
    let child = match child_service.find(&child_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.child_entity_name, display_id(&child_id)),
        Err(err) => return internal_error(err.to_string()),
    };
    (StatusCode::OK, Json(coerce_and_wrap(&state, child))).into_response()
}

async fn link_handler(
    State(state): State<RouterState>,
    Path((parent_raw, child_raw)): Path<(String, String)>,
) -> Response {
    let Some(child_service) = state.child_service.clone() else {
        return internal_error("link_handler mounted without child_service".to_string());
    };
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match parse_child_id(&state, &child_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child = match child_service.find(&child_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.child_entity_name, display_id(&child_id)),
        Err(err) => return internal_error(err.to_string()),
    };
    let all = match state.service.find_all().await {
        Ok(rows) => rows,
        Err(err) => return internal_error(err.to_string()),
    };
    let existing = find_junction(
        &all,
        state.parent_fk_field.as_str(),
        &parent_id,
        state.child_fk_field.as_str(),
        &child_id,
    );
    if existing.is_none() {
        let mut junction_body = RowMap::new();
        junction_body.insert(state.parent_fk_field.to_string(), parent_id.clone());
        junction_body.insert(state.child_fk_field.to_string(), child_id.clone());
        if let Err(err) = state.service.add(junction_body).await {
            return internal_error(err.to_string());
        }
    }
    let status = link_success_status(state.link_verb);
    (status, Json(coerce_and_wrap(&state, child))).into_response()
}

fn link_success_status(verb: LinkVerb) -> StatusCode {
    match verb {
        LinkVerb::Put => StatusCode::OK,
        LinkVerb::Post => StatusCode::CREATED,
    }
}

async fn patch_child_handler(
    State(state): State<RouterState>,
    Path((parent_raw, child_raw)): Path<(String, String)>,
    Json(body): Json<RowMap>,
) -> Response {
    let Some(child_service) = state.child_service.clone() else {
        return internal_error("patch_child_handler mounted without child_service".to_string());
    };
    let parent_id = match parse_parent_id(&state, &parent_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match parse_child_id(&state, &child_raw) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let all = match state.service.find_all().await {
        Ok(rows) => rows,
        Err(err) => return internal_error(err.to_string()),
    };
    let junction = find_junction(
        &all,
        state.parent_fk_field.as_str(),
        &parent_id,
        state.child_fk_field.as_str(),
        &child_id,
    );
    if junction.is_none() {
        return not_found(&state.child_entity_name, display_id(&child_id));
    }
    if let Err(resp) = run_validator(&state.patch_child_validator, &body) {
        return resp;
    }
    match child_service.update(&child_id, body, None).await {
        Ok(Some(row)) => (StatusCode::OK, Json(coerce_and_wrap(&state, row))).into_response(),
        Ok(None) => not_found(&state.child_entity_name, display_id(&child_id)),
        Err(err) => internal_error(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn junction(parent_id: i64, child_id: i64, id: i64) -> RowMap {
        let mut r = RowMap::new();
        r.insert("id".to_string(), Value::from(id));
        r.insert("user_id".to_string(), Value::from(parent_id));
        r.insert("role_id".to_string(), Value::from(child_id));
        r
    }

    #[test]
    fn filter_by_parent_fk_keeps_only_rows_with_matching_fk() {
        let rows = vec![
            junction(1, 10, 100),
            junction(2, 11, 101),
            junction(1, 12, 102),
        ];
        let out = filter_by_parent_fk(rows, "user_id", &Value::from(1i64));
        assert_eq!(out.len(), 2);
        for row in &out {
            assert_eq!(row.get("user_id").unwrap(), &Value::from(1i64));
        }
    }

    #[test]
    fn filter_by_parent_fk_returns_empty_when_no_rows_match() {
        let rows = vec![junction(1, 10, 100)];
        let out = filter_by_parent_fk(rows, "user_id", &Value::from(99i64));
        assert!(out.is_empty());
    }

    #[test]
    fn filter_by_parent_fk_treats_missing_fk_column_as_no_match() {
        let mut orphan = RowMap::new();
        orphan.insert("id".to_string(), Value::from(1i64));
        let out = filter_by_parent_fk(vec![orphan], "user_id", &Value::from(1i64));
        assert!(out.is_empty());
    }

    #[test]
    fn find_junction_returns_row_when_both_fks_match() {
        let rows = vec![junction(1, 10, 100), junction(1, 11, 101)];
        let out = find_junction(
            &rows,
            "user_id",
            &Value::from(1i64),
            "role_id",
            &Value::from(10i64),
        );
        assert!(out.is_some());
        assert_eq!(out.unwrap().get("id").unwrap(), &Value::from(100i64));
    }

    #[test]
    fn find_junction_returns_none_when_parent_fk_mismatches() {
        let rows = vec![junction(1, 10, 100)];
        let out = find_junction(
            &rows,
            "user_id",
            &Value::from(2i64),
            "role_id",
            &Value::from(10i64),
        );
        assert!(out.is_none());
    }

    #[test]
    fn find_junction_returns_none_when_child_fk_mismatches() {
        let rows = vec![junction(1, 10, 100)];
        let out = find_junction(
            &rows,
            "user_id",
            &Value::from(1i64),
            "role_id",
            &Value::from(11i64),
        );
        assert!(out.is_none());
    }

    #[test]
    fn junction_row_id_returns_id_column_when_present() {
        let j = junction(1, 10, 100);
        assert_eq!(junction_row_id(&j), Some(Value::from(100i64)));
    }

    #[test]
    fn junction_row_id_returns_none_when_id_column_missing() {
        let mut r = RowMap::new();
        r.insert("user_id".to_string(), Value::from(1i64));
        assert_eq!(junction_row_id(&r), None);
    }

    #[test]
    fn display_id_renders_number_bare_and_string_without_quotes() {
        assert_eq!(display_id(&Value::from(7i64)), "7");
        assert_eq!(display_id(&Value::String("k".to_string())), "k");
    }

    #[test]
    fn link_verb_default_is_put_matching_ts_parity() {
        assert_eq!(LinkVerb::default(), LinkVerb::Put);
    }

    #[test]
    fn link_success_status_returns_200_for_put() {
        assert_eq!(link_success_status(LinkVerb::Put), StatusCode::OK);
    }

    #[test]
    fn link_success_status_returns_201_for_post() {
        assert_eq!(link_success_status(LinkVerb::Post), StatusCode::CREATED);
    }

    use crate::error::RepositoryError;
    use async_trait::async_trait;

    struct NoopService;

    #[async_trait]
    impl CrudService for NoopService {
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

    fn state_for_mode_tests(
        child_service: Option<Arc<dyn CrudService>>,
        child_create_validator: Option<ValidatorFn>,
    ) -> RouterState {
        RouterState {
            service: Arc::new(NoopService),
            child_service,
            parent_entity_name: Arc::new("User".to_string()),
            child_entity_name: Arc::new("Role".to_string()),
            parent_param_name: Arc::new("user_id".to_string()),
            child_param_name: Arc::new("role_id".to_string()),
            parent_fk_field: Arc::new("user_id".to_string()),
            child_fk_field: Arc::new("role_id".to_string()),
            parent_id_type: IdType::Integer,
            child_id_type: IdType::Integer,
            create_validator: None,
            child_create_validator,
            patch_child_validator: None,
            coerce_row: None,
            link_verb: LinkVerb::Put,
            child_parent_fk_field: None,
        }
    }

    fn ok_validator() -> ValidatorFn {
        Arc::new(|_body: &RowMap| Ok(()))
    }

    #[test]
    fn is_create_and_link_mode_true_when_both_child_service_and_validator_are_set() {
        let state = state_for_mode_tests(Some(Arc::new(NoopService)), Some(ok_validator()));
        assert!(is_create_and_link_mode(&state));
    }

    #[test]
    fn is_create_and_link_mode_false_when_child_service_is_none() {
        let state = state_for_mode_tests(None, Some(ok_validator()));
        assert!(!is_create_and_link_mode(&state));
    }

    #[test]
    fn is_create_and_link_mode_false_when_child_create_validator_is_none() {
        let state = state_for_mode_tests(Some(Arc::new(NoopService)), None);
        assert!(!is_create_and_link_mode(&state));
    }

    #[test]
    fn is_create_and_link_mode_false_when_both_none() {
        let state = state_for_mode_tests(None, None);
        assert!(!is_create_and_link_mode(&state));
    }
}
