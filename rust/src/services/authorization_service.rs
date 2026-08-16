use async_trait::async_trait;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanAccessPermissionResult {
    pub granted: bool,
    pub reason: Option<String>,
    pub status_code: Option<u16>,
}

#[async_trait]
pub trait AuthorizationService: Send + Sync {
    async fn can_access_permission(
        &self,
        token: &str,
        permission: &str,
    ) -> CanAccessPermissionResult;
}
