use std::collections::HashMap;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::{Path, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{on, MethodFilter};
use axum::{Json, Router};
use serde_json::{Map, Value};

const REQUEST_BODY_LIMIT_BYTES: usize = 1024 * 1024;

use super::service_registry::ServiceRegistry;
use crate::error::RepositoryError;
use crate::loaders::{GenericRouteSpec, HttpMethod, ResponseFormat};
use crate::services::{DynamicService, ServiceError, IF_MATCH_ARG};

pub struct RouterBuildReport {
    pub registered: Vec<String>,
    pub skipped_missing_service: Vec<String>,
    // Axum-style paths ({param} placeholders, not :param) actually mounted — server.rs gates the runtime builtin /api/health on this so a YAML declaration whose service isn't registered (e.g. TS-only HealthCheckService in a Rust crate) doesn't mask the builtin.
    pub registered_paths: Vec<String>,
}

pub fn build_router(
    routes: &[GenericRouteSpec],
    registry: &ServiceRegistry,
) -> (Router, RouterBuildReport) {
    let mut router = Router::new();
    let mut report = RouterBuildReport {
        registered: Vec::new(),
        skipped_missing_service: Vec::new(),
        registered_paths: Vec::new(),
    };
    for spec in routes {
        let Some(service) = registry.get(&spec.service) else {
            report.skipped_missing_service.push(spec.route_name.clone());
            continue;
        };
        let method = http_method_to_axum_filter(&spec.method);
        let method_name = spec.service_method.clone();
        let axum_path = convert_path(&spec.path);
        let has_path_params = axum_path.contains('{');
        let svc = service.clone();
        let response_format = spec.response_format.clone();
        let status_code = spec
            .status_code
            .or_else(|| default_status_for(&spec.method));
        if has_path_params {
            let handler = move |Path(params): Path<HashMap<String, String>>, req: Request| {
                let svc = svc.clone();
                let method_name = method_name.clone();
                let rf = response_format.clone();
                let sc = status_code;
                async move { dispatch(svc, method_name, params, req, rf, sc).await }
            };
            router = router.route(&axum_path, on(method, handler));
        } else {
            let handler = move |req: Request| {
                let svc = svc.clone();
                let method_name = method_name.clone();
                let rf = response_format.clone();
                let sc = status_code;
                async move { dispatch(svc, method_name, HashMap::new(), req, rf, sc).await }
            };
            router = router.route(&axum_path, on(method, handler));
        }
        report.registered.push(spec.route_name.clone());
        report.registered_paths.push(axum_path);
    }
    (router, report)
}

fn http_method_to_axum_filter(m: &HttpMethod) -> MethodFilter {
    match m {
        HttpMethod::Get => MethodFilter::GET,
        HttpMethod::Post => MethodFilter::POST,
        HttpMethod::Put => MethodFilter::PUT,
        HttpMethod::Patch => MethodFilter::PATCH,
        HttpMethod::Delete => MethodFilter::DELETE,
    }
}

fn default_status_for(m: &HttpMethod) -> Option<u16> {
    match m {
        HttpMethod::Post => Some(201),
        _ => None,
    }
}

fn convert_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    let bytes = p.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            out.push('{');
            i += 1;
            while i < bytes.len() && bytes[i] != b'/' {
                out.push(bytes[i] as char);
                i += 1;
            }
            out.push('}');
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

async fn dispatch(
    service: Arc<dyn DynamicService>,
    method: String,
    path_params: HashMap<String, String>,
    req: Request,
    response_format: Option<ResponseFormat>,
    status_code: Option<u16>,
) -> Response {
    let (parts, body) = req.into_parts();
    let query_obj = match parts.uri.query() {
        Some(q) => parse_query_string(q),
        None => Map::new(),
    };
    let body_bytes = match to_bytes(body, REQUEST_BODY_LIMIT_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("body read failed: {}", e)).into_response();
        }
    };
    let body_value = if body_bytes.is_empty() {
        None
    } else {
        match serde_json::from_slice::<Value>(&body_bytes) {
            Ok(v) => Some(v),
            Err(_) => {
                let body = serde_json::json!({
                    "errors": [{ "code": "BAD_REQUEST", "message": "Malformed request body" }],
                });
                return (StatusCode::BAD_REQUEST, Json(body)).into_response();
            }
        }
    };
    let mut args = build_args(path_params, query_obj, body_value);
    if let (Some(if_match), Value::Object(map)) = (extract_if_match(&parts.headers), &mut args) {
        map.insert(IF_MATCH_ARG.to_string(), Value::String(if_match));
    }
    match service.invoke(&method, args).await {
        Ok(v) => {
            if v.is_null() && matches!(response_format, Some(ResponseFormat::Item)) {
                return not_found_response(&method);
            }
            let wrapped = apply_response_format(v, response_format.as_ref());
            let status = status_code
                .and_then(|c| StatusCode::from_u16(c).ok())
                .unwrap_or(StatusCode::OK);
            (status, Json(wrapped)).into_response()
        }
        Err(e) => service_error_to_response(e),
    }
}

fn not_found_response(method: &str) -> Response {
    let body = serde_json::json!({
        "errors": [{
            "code": "NOT_FOUND",
            "message": format!("resource not found ({})", method),
        }],
    });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

// Mirrors typescript/src/responses/sendResponse.ts — `items` wraps arrays in {items: [...]}, `item` and `raw` pass through. Single-resource handlers use `item`; list handlers use `items`; delete uses `raw` to pass through `{deleted: bool}`.
fn apply_response_format(v: Value, fmt: Option<&ResponseFormat>) -> Value {
    match fmt {
        Some(ResponseFormat::Items) => Value::Object({
            let mut m = Map::new();
            m.insert("items".to_string(), v);
            m
        }),
        _ => v,
    }
}

fn parse_query_string(q: &str) -> Map<String, Value> {
    let mut out = Map::new();
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut split = pair.splitn(2, '=');
        let k = split.next().unwrap_or("");
        let v = split.next().unwrap_or("");
        let k_decoded = urlencoding::decode(k)
            .map(|s| s.to_string())
            .unwrap_or_else(|_| k.to_string());
        let v_decoded = urlencoding::decode(v)
            .map(|s| s.to_string())
            .unwrap_or_else(|_| v.to_string());
        if !k_decoded.is_empty() {
            out.insert(k_decoded, Value::String(v_decoded));
        }
    }
    out
}

// Path params arrive as strings; postgres/mysql reject `WHERE id = $1` when $1 is bound as TEXT against an INTEGER column, so numeric-looking ids get coerced to JSON Number for the sqlx bind to match the column type. Non-numeric ids (uuid, string keys) stay strings. Shared with the combined/nested/m2m routers so every path-param id coerces one way.
pub(crate) fn coerce_path_param(v: &str) -> Value {
    if let Ok(n) = v.parse::<i64>() {
        return Value::Number(n.into());
    }
    Value::String(v.to_string())
}

fn build_args(
    path_params: HashMap<String, String>,
    query: Map<String, Value>,
    body: Option<Value>,
) -> Value {
    let mut args = Map::new();
    for (k, v) in path_params {
        args.insert(k, coerce_path_param(&v));
    }
    if !query.is_empty() {
        args.insert("query".to_string(), Value::Object(query));
    }
    if let Some(b) = body {
        if let Value::Object(obj) = &b {
            for (k, v) in obj {
                args.insert(k.clone(), v.clone());
            }
        }
        args.insert("body".to_string(), b);
    }
    Value::Object(args)
}

// If-Match arrives quoted per RFC 7232 (`If-Match: "etag"`); strip the quotes and reject blank so
// an empty header reads as absent. Mirrors the static CRUD router's require_if_match.
pub(crate) fn extract_if_match(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
}

fn service_error_to_response(err: ServiceError) -> Response {
    match &err {
        ServiceError::UnknownMethod(_) => {
            (StatusCode::METHOD_NOT_ALLOWED, err.to_string()).into_response()
        }
        ServiceError::InvalidArgs(_) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
        ServiceError::PreconditionRequired(_) => {
            (StatusCode::PRECONDITION_REQUIRED, err.to_string()).into_response()
        }
        ServiceError::Repository(RepositoryError::ConcurrencyConflict(_)) => {
            (StatusCode::PRECONDITION_FAILED, err.to_string()).into_response()
        }
        ServiceError::Repository(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
        ServiceError::Stub(body) => {
            (StatusCode::NOT_IMPLEMENTED, Json(body.clone())).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_path_handles_no_params() {
        assert_eq!(convert_path("/api/health"), "/api/health");
    }

    #[test]
    fn convert_path_rewrites_colon_params_to_braces() {
        assert_eq!(convert_path("/api/users/:id"), "/api/users/{id}");
        assert_eq!(
            convert_path("/api/users/:id/posts/:post_id"),
            "/api/users/{id}/posts/{post_id}"
        );
    }

    #[test]
    fn parse_query_string_decodes_simple_pairs() {
        let m = parse_query_string("a=1&b=two&c=hello%20world");
        assert_eq!(m["a"], Value::String("1".to_string()));
        assert_eq!(m["b"], Value::String("two".to_string()));
        assert_eq!(m["c"], Value::String("hello world".to_string()));
    }

    #[test]
    fn build_args_flattens_body_object_into_top_level() {
        let q = Map::new();
        let body = Some(serde_json::json!({ "name": "x", "age": 9 }));
        let args = build_args(HashMap::new(), q, body);
        assert_eq!(args["name"], Value::String("x".to_string()));
        assert_eq!(args["age"], Value::Number(9.into()));
        assert_eq!(args["body"]["name"], Value::String("x".to_string()));
    }

    #[test]
    fn build_args_omits_query_key_when_empty() {
        let args = build_args(HashMap::new(), Map::new(), None);
        let obj = args.as_object().unwrap();
        assert!(!obj.contains_key("query"));
        assert!(!obj.contains_key("body"));
    }

    #[test]
    fn build_args_coerces_numeric_path_params_to_number() {
        let mut p = HashMap::new();
        p.insert("id".to_string(), "42".to_string());
        let args = build_args(p, Map::new(), None);
        assert_eq!(args["id"], Value::Number(42.into()));
    }

    #[test]
    fn not_found_response_uses_404_and_errors_envelope() {
        let resp = not_found_response("findById");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn default_status_for_post_is_201_others_unset() {
        assert_eq!(default_status_for(&HttpMethod::Post), Some(201));
        assert_eq!(default_status_for(&HttpMethod::Get), None);
        assert_eq!(default_status_for(&HttpMethod::Put), None);
        assert_eq!(default_status_for(&HttpMethod::Patch), None);
        assert_eq!(default_status_for(&HttpMethod::Delete), None);
    }

    #[test]
    fn build_args_keeps_non_numeric_path_params_as_string() {
        let mut p = HashMap::new();
        p.insert("id".to_string(), "abc-123".to_string());
        let args = build_args(p, Map::new(), None);
        assert_eq!(args["id"], Value::String("abc-123".to_string()));
    }

    fn headers_with_if_match(raw: &str) -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("if-match", axum::http::HeaderValue::from_str(raw).unwrap());
        headers
    }

    #[test]
    fn extract_if_match_returns_none_when_header_absent() {
        assert_eq!(extract_if_match(&axum::http::HeaderMap::new()), None);
    }

    #[test]
    fn extract_if_match_strips_quotes_and_trims() {
        assert_eq!(
            extract_if_match(&headers_with_if_match("\"token-1\"")),
            Some("token-1".to_string())
        );
        assert_eq!(
            extract_if_match(&headers_with_if_match("  token-2  ")),
            Some("token-2".to_string())
        );
    }

    #[test]
    fn extract_if_match_treats_blank_header_as_absent() {
        assert_eq!(extract_if_match(&headers_with_if_match("   ")), None);
    }

    #[test]
    fn service_error_to_response_maps_precondition_required_to_428() {
        let resp = service_error_to_response(ServiceError::PreconditionRequired("x".to_string()));
        assert_eq!(resp.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[test]
    fn service_error_to_response_maps_concurrency_conflict_to_412() {
        let resp = service_error_to_response(ServiceError::Repository(
            RepositoryError::ConcurrencyConflict("stale".to_string()),
        ));
        assert_eq!(resp.status(), StatusCode::PRECONDITION_FAILED);
    }
}
