use std::sync::Arc;

use axum::Router;

use crate::error::RepositoryError;
use crate::services::DynamicService;
use crate::ServiceRegistry;

/// The hook a generated backend sets on [`super::RunConfig`] to compose the entity router itself: pull
/// each entity's composed service from the runtime registry (via [`ComposeContext::entity_service`])
/// and mount the generated router that routes to it. The runtime keeps the cross-cutting concerns
/// (health/statics/error/trace/serving) around whatever router the composer returns.
#[derive(Clone)]
pub struct RouteComposer(Arc<RouteComposerFn>);

type RouteComposerFn = dyn Fn(&ComposeContext) -> Result<Router, RepositoryError> + Send + Sync;

impl RouteComposer {
    pub fn new(
        compose: impl Fn(&ComposeContext) -> Result<Router, RepositoryError> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(compose))
    }

    pub fn compose(&self, ctx: &ComposeContext) -> Result<Router, RepositoryError> {
        (self.0)(ctx)
    }
}

impl std::fmt::Debug for RouteComposer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RouteComposer(<fn>)")
    }
}

/// Handed to a [`RouteComposer`]: exposes the runtime's composed service registry so a generated
/// service facade can pull its fully-composed stack (GenericCrudService → enrich → eager-write →
/// eager-read) by name, keeping the generated code a thin typed adapter over that stack.
pub struct ComposeContext {
    registry: ServiceRegistry,
}

impl ComposeContext {
    pub(crate) fn new(registry: ServiceRegistry) -> Self {
        Self { registry }
    }

    /// The composed service for an entity — the full decorator stack the runtime built for it.
    pub fn entity_service(
        &self,
        entity_name: &str,
    ) -> Result<Arc<dyn DynamicService>, RepositoryError> {
        self.registry.get(entity_name).ok_or_else(|| {
            RepositoryError::Other(format!(
                "compose: no service registered for entity `{}`",
                entity_name
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::inmemory::InMemoryCrudRepository;
    use crate::services::GenericCrudService;
    use crate::ServiceRegistryBuilder;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn registry_with(entity: &str) -> ServiceRegistry {
        let svc: Arc<dyn DynamicService> = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        ServiceRegistryBuilder::new().register(entity, svc).build()
    }

    #[test]
    fn entity_service_returns_the_registered_service() {
        let ctx = ComposeContext::new(registry_with("widget"));
        assert!(ctx.entity_service("widget").is_ok());
    }

    #[test]
    fn entity_service_errors_for_an_unregistered_entity() {
        let ctx = ComposeContext::new(registry_with("widget"));
        let err = match ctx.entity_service("nope") {
            Err(e) => e,
            Ok(_) => panic!("expected an error for an unregistered entity"),
        };
        assert!(format!("{}", err).contains("no service registered"));
    }

    #[test]
    fn route_composer_runs_and_returns_its_router() {
        let ctx = ComposeContext::new(registry_with("widget"));
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let composer = RouteComposer::new(move |_ctx| {
            flag.store(true, Ordering::SeqCst);
            Ok(Router::new())
        });
        let _router = composer.compose(&ctx).unwrap();
        assert!(called.load(Ordering::SeqCst), "composer closure must run");
    }
}
