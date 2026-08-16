use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{on, MethodFilter, MethodRouter};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::RepositoryError;
use crate::repositories::RowMap;
use crate::routes::crud::ValidatorFn;
use crate::routes::helpers::{internal_error, validation_errors_compound};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenericRouterMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GenericResponseFormat {
    #[default]
    Item,
    Items,
    Raw,
}

pub type GenericHandlerFuture =
    Pin<Box<dyn Future<Output = Result<Value, RepositoryError>> + Send + 'static>>;
pub type GenericHandlerFn = Arc<dyn Fn(RowMap) -> GenericHandlerFuture + Send + Sync + 'static>;

pub struct GenericRouterConfig {
    pub method: GenericRouterMethod,
    pub path: String,
    pub handler: GenericHandlerFn,
    pub request_validator: Option<ValidatorFn>,
    pub response_format: GenericResponseFormat,
    pub status_code: Option<u16>,
}

pub fn create_generic_router(cfg: GenericRouterConfig) -> Router {
    let state = RouterState {
        handler: cfg.handler,
        request_validator: cfg.request_validator,
        response_format: cfg.response_format,
        status_code: resolve_status_code(cfg.status_code, cfg.method, cfg.response_format),
    };
    let path = cfg.path;
    Router::new()
        .route(&path, method_router_for(cfg.method))
        .with_state(state)
}

fn method_router_for(method: GenericRouterMethod) -> MethodRouter<RouterState> {
    on(method_filter_for(method), dispatch_handler)
}

fn method_filter_for(method: GenericRouterMethod) -> MethodFilter {
    match method {
        GenericRouterMethod::Get => MethodFilter::GET,
        GenericRouterMethod::Post => MethodFilter::POST,
        GenericRouterMethod::Put => MethodFilter::PUT,
        GenericRouterMethod::Patch => MethodFilter::PATCH,
        GenericRouterMethod::Delete => MethodFilter::DELETE,
    }
}

fn resolve_status_code(
    override_code: Option<u16>,
    method: GenericRouterMethod,
    format: GenericResponseFormat,
) -> StatusCode {
    if let Some(code) = override_code {
        return StatusCode::from_u16(code).unwrap_or(StatusCode::OK);
    }
    match (method, format) {
        (GenericRouterMethod::Post, GenericResponseFormat::Item) => StatusCode::CREATED,
        _ => StatusCode::OK,
    }
}

#[derive(Clone)]
struct RouterState {
    handler: GenericHandlerFn,
    request_validator: Option<ValidatorFn>,
    response_format: GenericResponseFormat,
    status_code: StatusCode,
}

async fn dispatch_handler(
    State(state): State<RouterState>,
    body: Option<Json<RowMap>>,
) -> Response {
    let body_map = body.map(|Json(b)| b).unwrap_or_default();
    if let Err(resp) = run_validator(&state.request_validator, &body_map) {
        return resp;
    }
    let handler = state.handler.clone();
    let result = handler(body_map).await;
    match result {
        Ok(value) => wrap_response(value, state.response_format, state.status_code),
        Err(err) => internal_error(err.to_string()),
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

fn wrap_response(value: Value, format: GenericResponseFormat, status: StatusCode) -> Response {
    match format {
        GenericResponseFormat::Item => (status, Json(value)).into_response(),
        GenericResponseFormat::Items => {
            let items_body = json!({ "items": value });
            (status, Json(items_body)).into_response()
        }
        GenericResponseFormat::Raw => (status, Json(value)).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_filter_for_maps_every_method_to_its_axum_filter() {
        assert_eq!(
            method_filter_for(GenericRouterMethod::Get),
            MethodFilter::GET,
        );
        assert_eq!(
            method_filter_for(GenericRouterMethod::Post),
            MethodFilter::POST,
        );
        assert_eq!(
            method_filter_for(GenericRouterMethod::Put),
            MethodFilter::PUT,
        );
        assert_eq!(
            method_filter_for(GenericRouterMethod::Patch),
            MethodFilter::PATCH,
        );
        assert_eq!(
            method_filter_for(GenericRouterMethod::Delete),
            MethodFilter::DELETE,
        );
    }

    #[test]
    fn resolve_status_code_prefers_explicit_override() {
        assert_eq!(
            resolve_status_code(
                Some(202),
                GenericRouterMethod::Post,
                GenericResponseFormat::Item,
            ),
            StatusCode::ACCEPTED,
        );
    }

    #[test]
    fn resolve_status_code_returns_200_by_default() {
        for method in [
            GenericRouterMethod::Get,
            GenericRouterMethod::Put,
            GenericRouterMethod::Patch,
            GenericRouterMethod::Delete,
        ] {
            assert_eq!(
                resolve_status_code(None, method, GenericResponseFormat::Item),
                StatusCode::OK,
            );
        }
        for method in [
            GenericRouterMethod::Post,
            GenericRouterMethod::Get,
            GenericRouterMethod::Put,
        ] {
            assert_eq!(
                resolve_status_code(None, method, GenericResponseFormat::Items),
                StatusCode::OK,
            );
        }
    }

    #[test]
    fn resolve_status_code_returns_201_for_post_with_item_format() {
        assert_eq!(
            resolve_status_code(None, GenericRouterMethod::Post, GenericResponseFormat::Item,),
            StatusCode::CREATED,
        );
    }

    #[test]
    fn resolve_status_code_returns_200_for_post_with_items_format() {
        assert_eq!(
            resolve_status_code(
                None,
                GenericRouterMethod::Post,
                GenericResponseFormat::Items,
            ),
            StatusCode::OK,
        );
    }

    #[test]
    fn resolve_status_code_returns_200_for_post_with_raw_format() {
        assert_eq!(
            resolve_status_code(None, GenericRouterMethod::Post, GenericResponseFormat::Raw,),
            StatusCode::OK,
        );
    }

    #[test]
    fn resolve_status_code_falls_back_to_ok_for_out_of_range_override() {
        assert_eq!(
            resolve_status_code(
                Some(9999),
                GenericRouterMethod::Get,
                GenericResponseFormat::Item,
            ),
            StatusCode::OK,
        );
    }

    #[test]
    fn wrap_response_item_returns_raw_value_body() {
        let resp = wrap_response(
            json!({ "id": 1, "name": "x" }),
            GenericResponseFormat::Item,
            StatusCode::OK,
        );
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn wrap_response_items_wraps_in_items_envelope() {
        let resp = wrap_response(
            json!([{ "id": 1 }, { "id": 2 }]),
            GenericResponseFormat::Items,
            StatusCode::OK,
        );
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn wrap_response_raw_returns_status_and_json_body_as_is() {
        let resp = wrap_response(
            json!({ "any": "shape" }),
            GenericResponseFormat::Raw,
            StatusCode::ACCEPTED,
        );
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    #[test]
    fn run_validator_returns_ok_when_hook_is_none() {
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

    #[test]
    fn generic_response_format_default_is_item() {
        assert_eq!(
            GenericResponseFormat::default(),
            GenericResponseFormat::Item
        );
    }
}
