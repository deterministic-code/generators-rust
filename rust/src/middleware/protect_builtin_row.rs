use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::header::CONTENT_LENGTH;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::middleware::error_handler::AppError;
use crate::middleware::json_body_middleware::JSON_BODY_LIMIT_BYTES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltinRow {
    pub id: i64,
    pub is_builtin: bool,
    pub name: String,
}

#[async_trait]
pub trait BuiltinRowReader: Send + Sync {
    async fn find_by_id(&self, id: i64) -> Option<BuiltinRow>;
}

#[derive(Clone)]
pub struct ProtectBuiltinRowState {
    pub reader: Arc<dyn BuiltinRowReader>,
    pub entity_name: String,
}

pub struct ProtectBuiltinRow {
    reader: Arc<dyn BuiltinRowReader>,
    entity_name: String,
}

impl ProtectBuiltinRow {
    pub fn new(reader: Arc<dyn BuiltinRowReader>, entity_name: impl Into<String>) -> Self {
        Self {
            reader,
            entity_name: entity_name.into(),
        }
    }

    pub fn state(&self) -> ProtectBuiltinRowState {
        ProtectBuiltinRowState {
            reader: self.reader.clone(),
            entity_name: self.entity_name.clone(),
        }
    }
}

pub async fn protect_builtin_row_layer(
    State(state): State<ProtectBuiltinRowState>,
    path: Option<Path<i64>>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().clone();
    if method != axum::http::Method::PUT
        && method != axum::http::Method::PATCH
        && method != axum::http::Method::DELETE
    {
        return next.run(req).await;
    }

    let id = match path {
        Some(Path(v)) if v > 0 => v,
        _ => return next.run(req).await,
    };

    let entity = match state.reader.find_by_id(id).await {
        Some(e) => e,
        None => return next.run(req).await,
    };

    if !entity.is_builtin {
        return next.run(req).await;
    }

    if method == axum::http::Method::DELETE {
        return forbidden(&format!(
            "Cannot delete {} because it's a builtin row",
            state.entity_name
        ));
    }

    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, JSON_BODY_LIMIT_BYTES).await {
        Ok(b) => b,
        Err(_) => {
            return AppError::Custom {
                status: 413,
                code: "PAYLOAD_TOO_LARGE".to_string(),
                message: "Request body exceeded limit".to_string(),
            }
            .into_response()
        }
    };

    if bytes.is_empty() {
        let req = Request::from_parts(parts, Body::from(bytes));
        return next.run(req).await;
    }

    let mut json: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            let req = Request::from_parts(parts, Body::from(bytes));
            return next.run(req).await;
        }
    };

    if let Some(obj) = json.as_object_mut() {
        if let Some(name_value) = obj.get("name") {
            if let Some(new_name) = name_value.as_str() {
                if new_name != entity.name {
                    return forbidden(&format!(
                        "Cannot rename {} because it's a builtin row",
                        state.entity_name
                    ));
                }
            }
        }
        obj.remove("is_builtin");
    }

    let new_bytes = match serde_json::to_vec(&json) {
        Ok(b) => b,
        Err(_) => return AppError::Internal.into_response(),
    };

    let mut parts = parts;
    parts.headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&new_bytes.len().to_string()).unwrap(),
    );
    let req = Request::from_parts(parts, Body::from(new_bytes));
    next.run(req).await
}

fn forbidden(message: &str) -> Response {
    AppError::Custom {
        status: 403,
        code: "FORBIDDEN".to_string(),
        message: message.to_string(),
    }
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::extract::Request as ExReq;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::{delete, patch, put};
    use axum::Router;
    use http_body_util::BodyExt;
    use std::sync::Mutex;
    use tower::ServiceExt;

    struct StubReader {
        builtin_ids: Vec<i64>,
        builtin_name: String,
    }
    #[async_trait]
    impl BuiltinRowReader for StubReader {
        async fn find_by_id(&self, id: i64) -> Option<BuiltinRow> {
            if self.builtin_ids.contains(&id) {
                Some(BuiltinRow {
                    id,
                    is_builtin: true,
                    name: self.builtin_name.clone(),
                })
            } else if id > 0 {
                Some(BuiltinRow {
                    id,
                    is_builtin: false,
                    name: format!("row-{}", id),
                })
            } else {
                None
            }
        }
    }

    #[derive(Clone, Default)]
    struct BodyProbe {
        last_body: Arc<Mutex<Option<serde_json::Value>>>,
    }

    fn app_with_probe(builtin_ids: Vec<i64>, builtin_name: &str, probe: BodyProbe) -> Router {
        let reader: Arc<dyn BuiltinRowReader> = Arc::new(StubReader {
            builtin_ids,
            builtin_name: builtin_name.to_string(),
        });
        let state = ProtectBuiltinRowState {
            reader,
            entity_name: "users".to_string(),
        };

        let capture_probe = probe.clone();
        let capture = move |req: ExReq| {
            let probe = capture_probe.clone();
            async move {
                let bytes = req.into_body().collect().await.unwrap().to_bytes();
                let parsed: serde_json::Value = if bytes.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
                };
                *probe.last_body.lock().unwrap() = Some(parsed);
                "handled".to_string()
            }
        };

        Router::new()
            .route("/api/users/{id}", delete(|| async { "deleted" }))
            .route("/api/users/{id}", patch(capture.clone()))
            .route("/api/users/{id}", put(capture))
            .layer(axum::middleware::from_fn_with_state(
                state,
                protect_builtin_row_layer,
            ))
    }

    fn app(builtin_ids: Vec<i64>) -> Router {
        app_with_probe(builtin_ids, "row-1", BodyProbe::default())
    }

    async fn body_string(res: Response) -> String {
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn delete_on_builtin_row_returns_403_forbidden_envelope() {
        let req = HttpRequest::builder()
            .method("DELETE")
            .uri("/api/users/1")
            .body(Body::empty())
            .unwrap();
        let res = app(vec![1]).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let body = body_string(res).await;
        assert!(body.contains("FORBIDDEN"), "{}", body);
        assert!(
            body.contains("Cannot delete users because it's a builtin row"),
            "{}",
            body
        );
    }

    #[tokio::test]
    async fn delete_on_non_builtin_row_passes_through() {
        let req = HttpRequest::builder()
            .method("DELETE")
            .uri("/api/users/2")
            .body(Body::empty())
            .unwrap();
        let res = app(vec![1]).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn patch_on_builtin_with_empty_body_passes_through() {
        let req = HttpRequest::builder()
            .method("PATCH")
            .uri("/api/users/1")
            .body(Body::empty())
            .unwrap();
        let res = app(vec![1]).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn patch_on_builtin_row_with_rename_returns_403() {
        let req = HttpRequest::builder()
            .method("PATCH")
            .uri("/api/users/1")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"renamed"}"#))
            .unwrap();
        let res = app_with_probe(vec![1], "row-1", BodyProbe::default())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let body = body_string(res).await;
        assert!(
            body.contains("Cannot rename users because it's a builtin row"),
            "{}",
            body
        );
    }

    #[tokio::test]
    async fn put_on_builtin_row_with_rename_returns_403() {
        let req = HttpRequest::builder()
            .method("PUT")
            .uri("/api/users/1")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"renamed","description":"x"}"#))
            .unwrap();
        let res = app_with_probe(vec![1], "row-1", BodyProbe::default())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let body = body_string(res).await;
        assert!(
            body.contains("Cannot rename users because it's a builtin row"),
            "{}",
            body
        );
    }

    #[tokio::test]
    async fn patch_on_builtin_row_with_same_name_passes_through() {
        let req = HttpRequest::builder()
            .method("PATCH")
            .uri("/api/users/1")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"row-1","description":"x"}"#))
            .unwrap();
        let res = app_with_probe(vec![1], "row-1", BodyProbe::default())
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn patch_on_builtin_row_strips_is_builtin_from_body() {
        let probe = BodyProbe::default();
        let router = app_with_probe(vec![1], "row-1", probe.clone());
        let req = HttpRequest::builder()
            .method("PATCH")
            .uri("/api/users/1")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"description":"x","is_builtin":false}"#))
            .unwrap();
        let res = router.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let captured = probe.last_body.lock().unwrap().clone().unwrap();
        assert!(
            captured.get("is_builtin").is_none(),
            "is_builtin should be stripped: {:?}",
            captured
        );
        assert_eq!(
            captured.get("description").and_then(|v| v.as_str()),
            Some("x")
        );
    }

    #[tokio::test]
    async fn patch_on_non_builtin_row_does_not_strip_is_builtin() {
        let probe = BodyProbe::default();
        let router = app_with_probe(vec![1], "row-1", probe.clone());
        let req = HttpRequest::builder()
            .method("PATCH")
            .uri("/api/users/2")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"description":"x","is_builtin":true}"#))
            .unwrap();
        let res = router.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let captured = probe.last_body.lock().unwrap().clone().unwrap();
        assert_eq!(
            captured.get("is_builtin").and_then(|v| v.as_bool()),
            Some(true),
            "non-builtin rows keep is_builtin: {:?}",
            captured
        );
    }
}
