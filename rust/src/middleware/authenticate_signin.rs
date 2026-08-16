use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use crate::middleware::error_handler::AppError;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SigninSessionInfo {
    pub uuid: String,
    pub session_id: i64,
    pub token: String,
}

#[derive(Debug, Deserialize)]
struct SigninClaims {
    sub: Option<String>,
    #[serde(rename = "type")]
    typ: Option<String>,
    session_id: Option<i64>,
}

pub struct AuthenticateSignin {
    jwt_secret: String,
}

impl AuthenticateSignin {
    pub fn new(jwt_secret: impl Into<String>) -> Self {
        Self {
            jwt_secret: jwt_secret.into(),
        }
    }

    pub fn jwt_secret(&self) -> &str {
        &self.jwt_secret
    }
}

pub async fn authenticate_signin_layer(
    State(jwt_secret): State<String>,
    mut req: Request,
    next: Next,
) -> Response {
    let auth_header = match req.headers().get(axum::http::header::AUTHORIZATION) {
        Some(h) => h.to_str().unwrap_or("").to_string(),
        None => {
            return signin_error(
                401,
                "SIGNIN_SESSION_REQUIRED",
                "Authorization header is required",
            )
        }
    };

    if !auth_header.starts_with("Bearer ") {
        return signin_error(
            401,
            "SIGNIN_SESSION_REQUIRED",
            "Authorization header must use Bearer scheme",
        );
    }

    let token = auth_header[7..].trim().to_string();
    if token.is_empty() {
        return signin_error(
            401,
            "SIGNIN_SESSION_REQUIRED",
            "Signin session token is required",
        );
    }

    let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.required_spec_claims.clear();
    let claims = match decode::<SigninClaims>(
        &token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &validation,
    ) {
        Ok(d) => d.claims,
        Err(_) => {
            return signin_error(
                401,
                "SIGNIN_SESSION_INVALID",
                "Invalid signin session token",
            )
        }
    };

    if claims.typ.as_deref() != Some("signin") {
        return signin_error(
            401,
            "SIGNIN_SESSION_INVALID",
            "Token is not a signin session token",
        );
    }

    let sub = match claims.sub {
        Some(s) => s,
        None => {
            return signin_error(
                401,
                "SIGNIN_SESSION_INVALID",
                "Token is missing session identifier",
            )
        }
    };

    let info = SigninSessionInfo {
        uuid: sub,
        session_id: claims.session_id.unwrap_or(0),
        token,
    };
    req.extensions_mut().insert(info);
    next.run(req).await
}

fn signin_error(status: u16, code: &str, message: &str) -> Response {
    AppError::Business {
        status_code: status,
        code: code.to_string(),
        message: message.to_string(),
    }
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::Serialize;
    use tower::ServiceExt;

    const SECRET: &str = "test-secret";

    #[derive(Serialize)]
    struct EncClaims<'a> {
        sub: &'a str,
        #[serde(rename = "type")]
        typ: &'a str,
        session_id: i64,
    }

    #[derive(Serialize)]
    struct EncClaimsWithExp<'a> {
        sub: &'a str,
        #[serde(rename = "type")]
        typ: &'a str,
        session_id: i64,
        exp: i64,
    }

    fn sign(typ: &str, sub: &str, session_id: i64) -> String {
        encode(
            &Header::default(),
            &EncClaims {
                sub,
                typ,
                session_id,
            },
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap()
    }

    fn sign_with_exp(typ: &str, sub: &str, session_id: i64, exp: i64) -> String {
        encode(
            &Header::default(),
            &EncClaimsWithExp {
                sub,
                typ,
                session_id,
                exp,
            },
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap()
    }

    fn app() -> Router {
        Router::new()
            .route(
                "/x",
                get(
                    |axum::Extension(s): axum::Extension<SigninSessionInfo>| async move {
                        format!("uuid:{}:sid:{}", s.uuid, s.session_id)
                    },
                ),
            )
            .layer(axum::middleware::from_fn_with_state(
                SECRET.to_string(),
                authenticate_signin_layer,
            ))
    }

    async fn body_string(res: Response) -> String {
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    }

    #[tokio::test]
    async fn valid_signin_jwt_is_decoded_and_attached_to_request() {
        let token = sign("signin", "abc-123", 42);
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_string(res).await;
        assert_eq!(body, "uuid:abc-123:sid:42");
    }

    #[tokio::test]
    async fn jwt_with_wrong_type_returns_401_invalid_token_envelope() {
        let token = sign("access", "abc", 1);
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = body_string(res).await;
        assert!(body.contains("SIGNIN_SESSION_INVALID"), "{}", body);
        assert!(body.contains("not a signin session token"), "{}", body);
    }

    #[tokio::test]
    async fn missing_auth_header_returns_signin_session_required() {
        let req = HttpRequest::builder()
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = body_string(res).await;
        assert!(body.contains("SIGNIN_SESSION_REQUIRED"), "{}", body);
    }

    #[tokio::test]
    async fn malformed_token_returns_signin_session_invalid() {
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", "Bearer not-a-jwt")
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = body_string(res).await;
        assert!(body.contains("SIGNIN_SESSION_INVALID"), "{}", body);
    }

    #[tokio::test]
    async fn expired_token_returns_signin_session_invalid() {
        let past = chrono::Utc::now().timestamp() - 3600;
        let token = sign_with_exp("signin", "session-uuid", 1, past);
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = body_string(res).await;
        assert!(body.contains("SIGNIN_SESSION_INVALID"), "{}", body);
    }

    #[tokio::test]
    async fn token_without_exp_claim_is_still_accepted() {
        let token = sign("signin", "session-uuid", 7);
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn token_with_future_exp_claim_is_accepted() {
        let future = chrono::Utc::now().timestamp() + 3600;
        let token = sign_with_exp("signin", "session-uuid", 42, future);
        let req = HttpRequest::builder()
            .uri("/x")
            .header("authorization", format!("Bearer {}", token))
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
