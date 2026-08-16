use std::sync::Arc;

use crate::services::DynamicService;

/// Custom service registrations the consumer's main.rs hands to `RunConfig::custom_services`; create-services patches the emitted registration block.
#[derive(Clone, Default)]
pub struct CustomServices {
    entries: Vec<(String, Arc<dyn DynamicService>)>,
}

impl CustomServices {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: &str, service: Arc<dyn DynamicService>) -> Self {
        self.entries.push((name.to_string(), service));
        self
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(name, _)| name.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, Arc<dyn DynamicService>)> {
        self.entries.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::fmt::Debug for CustomServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ServiceError;
    use serde_json::Value;

    struct FakeService;

    #[async_trait::async_trait]
    impl DynamicService for FakeService {
        async fn invoke(&self, _method: &str, _args: Value) -> Result<Value, ServiceError> {
            Ok(Value::Null)
        }
    }

    #[test]
    fn default_is_empty() {
        let services = CustomServices::new();
        assert!(services.is_empty());
        assert_eq!(services.names().count(), 0);
    }

    #[test]
    fn with_registers_in_order_and_names_reflect_entries() {
        let services = CustomServices::new()
            .with("ReportService", Arc::new(FakeService))
            .with("AuditService", Arc::new(FakeService));
        assert!(!services.is_empty());
        let names: Vec<&str> = services.names().collect();
        assert_eq!(names, vec!["ReportService", "AuditService"]);
        assert_eq!(services.iter().count(), 2);
    }

    #[test]
    fn debug_prints_names_only() {
        let services = CustomServices::new().with("ReportService", Arc::new(FakeService));
        assert_eq!(format!("{services:?}"), "[\"ReportService\"]");
    }
}
