use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::middleware::error_handler::AppError;
use crate::services::authorization_service::AuthorizationService;

pub struct Authorize {
    svc: Arc<dyn AuthorizationService>,
}

impl Authorize {
    pub fn new(svc: Arc<dyn AuthorizationService>) -> Self {
        Self { svc }
    }

    pub fn state(&self) -> Arc<dyn AuthorizationService> {
        self.svc.clone()
    }
}

pub fn derive_permission(method: &str, path: &str) -> String {
    let verb = match method.to_uppercase().as_str() {
        "POST" => "create".to_string(),
        "GET" => "read".to_string(),
        "PUT" | "PATCH" => "update".to_string(),
        "DELETE" => "delete".to_string(),
        other => other.to_lowercase(),
    };
    let without_prefix = path.strip_prefix("/api/").unwrap_or(path);
    let first_segment = without_prefix.split('/').next().unwrap_or("");
    let snake_case = first_segment.replace('-', "_");
    let singular = singularize(&snake_case);
    format!("{}:{}", verb, singular)
}

fn singularize(word: &str) -> String {
    if word.ends_with("ies") {
        let mut s = word[..word.len() - 3].to_string();
        s.push('y');
        return s;
    }
    if word.ends_with("sses") || word.ends_with("shes") || word.ends_with("xes") {
        return word[..word.len() - 2].to_string();
    }
    if word.ends_with('s') {
        return word[..word.len() - 1].to_string();
    }
    word.to_string()
}

pub async fn authorize_layer(
    State(svc): State<Arc<dyn AuthorizationService>>,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = match req.headers().get(axum::http::header::AUTHORIZATION) {
        Some(h) => h.to_str().unwrap_or("").to_string(),
        None => {
            return AppError::Business {
                status_code: 401,
                code: "BUSINESS_ERROR".to_string(),
                message: "Authorization header is required".to_string(),
            }
            .into_response();
        }
    };

    let parts: Vec<&str> = auth_header.split(' ').collect();
    if parts.first().copied() != Some("Bearer") {
        return AppError::Business {
            status_code: 401,
            code: "BUSINESS_ERROR".to_string(),
            message: "Authorization header must use Bearer scheme".to_string(),
        }
        .into_response();
    }

    let token = parts[1..].join(" ").trim().to_string();
    if token.is_empty() {
        return AppError::Business {
            status_code: 401,
            code: "BUSINESS_ERROR".to_string(),
            message: "Token is required".to_string(),
        }
        .into_response();
    }

    let permission = derive_permission(req.method().as_str(), req.uri().path());
    let result = svc.can_access_permission(&token, &permission).await;
    if result.granted {
        return next.run(req).await;
    }
    AppError::Business {
        status_code: result.status_code.unwrap_or(403),
        code: "BUSINESS_ERROR".to_string(),
        message: result.reason.unwrap_or_else(|| "Forbidden".to_string()),
    }
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::authorization_service::CanAccessPermissionResult;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use std::sync::Mutex;
    use tower::ServiceExt;

    #[test]
    fn derive_permission_singularizes_plural_resources() {
        assert_eq!(derive_permission("GET", "/api/projects"), "read:project");
        assert_eq!(
            derive_permission("DELETE", "/api/token-hits/42"),
            "delete:token_hit"
        );
        assert_eq!(derive_permission("POST", "/api/entries"), "create:entry");
        assert_eq!(derive_permission("PUT", "/api/classes"), "update:class");
    }

    #[test]
    fn derive_permission_maps_http_methods_to_crud_verbs() {
        assert_eq!(derive_permission("GET", "/api/x"), "read:x");
        assert_eq!(derive_permission("POST", "/api/x"), "create:x");
        assert_eq!(derive_permission("PUT", "/api/x"), "update:x");
        assert_eq!(derive_permission("PATCH", "/api/x"), "update:x");
        assert_eq!(derive_permission("DELETE", "/api/x"), "delete:x");
        assert_eq!(derive_permission("OPTIONS", "/api/x"), "options:x");
    }

    struct StubAuthz {
        granted_perm: String,
        seen: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AuthorizationService for StubAuthz {
        async fn can_access_permission(
            &self,
            _token: &str,
            permission: &str,
        ) -> CanAccessPermissionResult {
            self.seen.lock().unwrap().push(permission.to_string());
            CanAccessPermissionResult {
                granted: permission == self.granted_perm,
                reason: if permission == self.granted_perm {
                    None
                } else {
                    Some("not allowed".to_string())
                },
                status_code: None,
            }
        }
    }

    async fn body_string(res: Response) -> String {
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    }

    fn build_app(stub: Arc<StubAuthz>) -> Router {
        let svc: Arc<dyn AuthorizationService> = stub;
        Router::new()
            .route("/api/projects", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(svc, authorize_layer))
    }

    #[tokio::test]
    async fn granted_permission_calls_handler() {
        let stub = Arc::new(StubAuthz {
            granted_perm: "read:project".to_string(),
            seen: Mutex::new(Vec::new()),
        });
        let app = build_app(stub.clone());
        let req = HttpRequest::builder()
            .uri("/api/projects")
            .header("authorization", "Bearer tok")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let seen = stub.seen.lock().unwrap();
        assert_eq!(*seen, vec!["read:project".to_string()]);
    }

    #[tokio::test]
    async fn denied_permission_returns_403_forbidden_envelope() {
        let stub = Arc::new(StubAuthz {
            granted_perm: "write:project".to_string(),
            seen: Mutex::new(Vec::new()),
        });
        let app = build_app(stub);
        let req = HttpRequest::builder()
            .uri("/api/projects")
            .header("authorization", "Bearer tok")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let body = body_string(res).await;
        assert!(body.contains("not allowed"), "{}", body);
    }

    #[tokio::test]
    async fn missing_bearer_returns_401_business_error() {
        let stub = Arc::new(StubAuthz {
            granted_perm: "read:project".to_string(),
            seen: Mutex::new(Vec::new()),
        });
        let app = build_app(stub);
        let req = HttpRequest::builder()
            .uri("/api/projects")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
