use serde_json::{json, Value};

pub struct HealthCheckService;

impl HealthCheckService {
    pub fn new() -> Self {
        Self
    }

    pub async fn check(&self) -> Value {
        json!({ "status": "ok" })
    }
}
