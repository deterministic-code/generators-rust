use std::collections::HashMap;
use std::sync::Arc;

use crate::services::DynamicService;

#[derive(Clone, Default)]
pub struct ServiceRegistry {
    services: Arc<HashMap<String, Arc<dyn DynamicService>>>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_map(map: HashMap<String, Arc<dyn DynamicService>>) -> Self {
        Self {
            services: Arc::new(map),
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn DynamicService>> {
        self.services.get(name).cloned()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.services.keys().map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.services.len()
    }

    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
}

pub struct ServiceRegistryBuilder {
    services: HashMap<String, Arc<dyn DynamicService>>,
}

impl ServiceRegistryBuilder {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    pub fn register(mut self, name: &str, service: Arc<dyn DynamicService>) -> Self {
        self.services.insert(name.to_string(), service);
        self
    }

    pub fn build(self) -> ServiceRegistry {
        ServiceRegistry::from_map(self.services)
    }
}

impl Default for ServiceRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}
