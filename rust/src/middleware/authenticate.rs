use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::middleware::error_handler::AppError;
use crate::services::authentication_service::AuthenticationService;

pub struct Authenticate {
    svc: Arc<dyn AuthenticationService>,
}

impl Authenticate {
    pub fn new(svc: Arc<dyn AuthenticationService>) -> Self {
        Self { svc }
    }

    pub fn state(&self) -> Arc<dyn AuthenticationService> {
        self.svc.clone()
    }
}

pub async fn authenticate_layer(
    State(svc): State<Arc<dyn AuthenticationService>>,
    mut req: Request,
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

    match svc.authenticate(&token).await {
        Ok(user_info) => {
            req.extensions_mut().insert(user_info);
            next.run(req).await
        }
        Err(err) => AppError::Business {
            status_code: 401,
            code: "BUSINESS_ERROR".to_string(),
            message: err.to_string(),
        }
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::user_info::UserInfoResult;
    use crate::services::AuthError;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    struct StubAuth;
    #[async_trait]
    impl AuthenticationService for StubAuth {
        async fn authenticate(&self, token: &str) -> Result<UserInfoResult, AuthError> {
            if token == "valid" {
                Ok(UserInfoResult {
                    user_id: 1,
                    username: "u".to_string(),
                    roles: vec![],
                    scopes: vec![],
                    iat: 0,
                    exp: 0,
                })
            } else {
                Err(AuthError::InvalidToken("nope".to_string()))
            }
        }
    }

    fn app() -> Router {
        let svc: Arc<dyn AuthenticationService> = Arc::new(StubAuth);
        Router::new()
            .route(
                "/x",
                get(
                    |axum::Extension(user): axum::Extension<UserInfoResult>| async move {
                        format!("user:{}", user.user_id)
                    },
                ),
            )
            .layer(axum::middleware::from_fn_with_state(
                svc,
                authenticate_layer,
            ))
    }

    async fn body_string(res: Response) -> String {
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401_with_business_error_envelope() {
        let req = HttpRequest::builder()
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = body_string(res).await;
        assert!(
            body.contains("Authorization header is required"),
            "{}",
            body
        );
    }

    #[tokio::test]
    async fn non_bearer_scheme_returns_401() {
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", "Basic abc")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = body_string(res).await;
        assert!(body.contains("Bearer"), "{}", body);
    }

    #[tokio::test]
    async fn empty_bearer_token_returns_401() {
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", "Bearer ")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = body_string(res).await;
        assert!(body.contains("Token is required"), "{}", body);
    }

    #[tokio::test]
    async fn invalid_token_returns_401() {
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", "Bearer bad")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_token_injects_user_and_calls_handler() {
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", "Bearer valid")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_string(res).await;
        assert_eq!(body, "user:1");
    }
}
