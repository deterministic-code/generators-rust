pub mod dynamic;
pub mod service_registry;

pub use dynamic::{build_router, RouterBuildReport};
pub use service_registry::{ServiceRegistry, ServiceRegistryBuilder};
