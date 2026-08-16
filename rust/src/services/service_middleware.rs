use async_trait::async_trait;
use serde_json::Value;

use super::dynamic_service::ServiceError;

#[async_trait]
pub trait ServiceMiddleware: Send + Sync {
    async fn before_call(&self, service_name: &str, method_name: &str, args: &Value);

    async fn after_call(
        &self,
        service_name: &str,
        method_name: &str,
        args: &Value,
        result: &Result<Value, ServiceError>,
        elapsed_ms: f64,
    );
}
