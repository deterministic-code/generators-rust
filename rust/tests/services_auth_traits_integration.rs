use async_trait::async_trait;
use deterministic::{
    AuthError, AuthenticationService, AuthorizationService, CanAccessPermissionResult,
    UserInfoResult,
};
use std::sync::Arc;

struct StubAuth;
#[async_trait]
impl AuthenticationService for StubAuth {
    async fn authenticate(&self, access_token: &str) -> Result<UserInfoResult, AuthError> {
        if access_token == "valid" {
            Ok(UserInfoResult {
                user_id: 7,
                username: "alice".to_string(),
                roles: vec!["admin".to_string()],
                scopes: vec!["read".to_string(), "write".to_string()],
                iat: 1_700_000_000,
                exp: 1_800_000_000,
            })
        } else {
            Err(AuthError::InvalidToken("bad".to_string()))
        }
    }
}

struct StubAuthz;
#[async_trait]
impl AuthorizationService for StubAuthz {
    async fn can_access_permission(
        &self,
        _token: &str,
        permission: &str,
    ) -> CanAccessPermissionResult {
        CanAccessPermissionResult {
            granted: permission == "read:project",
            reason: if permission == "read:project" {
                None
            } else {
                Some("not allowed".to_string())
            },
            status_code: None,
        }
    }
}

#[tokio::test]
async fn authentication_service_trait_is_object_safe_via_arc_dyn() {
    let svc: Arc<dyn AuthenticationService> = Arc::new(StubAuth);
    let user = svc.authenticate("valid").await.expect("ok");
    assert_eq!(user.user_id, 7);
    assert_eq!(user.username, "alice");
}

#[tokio::test]
async fn authentication_service_returns_invalid_token_error_for_bad_token() {
    let svc = StubAuth;
    let err = svc.authenticate("nope").await.unwrap_err();
    assert!(matches!(err, AuthError::InvalidToken(_)));
}

#[tokio::test]
async fn authorization_service_trait_is_object_safe_via_arc_dyn() {
    let svc: Arc<dyn AuthorizationService> = Arc::new(StubAuthz);
    let granted = svc.can_access_permission("token", "read:project").await;
    assert!(granted.granted);
    assert!(granted.reason.is_none());

    let denied = svc.can_access_permission("token", "delete:project").await;
    assert!(!denied.granted);
    assert_eq!(denied.reason.unwrap(), "not allowed");
}
