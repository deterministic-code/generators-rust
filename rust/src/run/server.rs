use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use thiserror::Error;
use tokio::net::TcpListener;

use crate::backend_app_config::{
    apply_enable_middleware, is_handler_enabled, load_backend_app_config,
    parse_enable_middleware_env, BackendAppConfig, BackendAppConfigError, MiddlewareEntry,
};
use crate::error::RepositoryError;
use crate::loaders::{
    load_datasource_types, load_routes, load_services, load_settings_config, load_view_types,
    DatasourceTypesError, RoutesError, ServiceSpec, ServicesError, SettingsConfigError,
    ViewTypesError,
};
use crate::middleware::{
    trace_route_layer, DataSourceMiddleware, DataSourceMiddlewareLookup, LookupError,
    ServiceMiddlewareLookup, TraceRouteMiddleware,
};
use crate::services::{CustomServices, DynamicService, ServiceMiddleware, ServiceWithMiddleware};
use crate::{build_router, ServiceRegistry, ServiceRegistryBuilder};

use super::compose::ComposeContext;
use super::config::RunConfig;
use super::datasource::{open_datasource, OpenDatasource};

#[derive(Debug, Error)]
pub enum RunError {
    #[error("repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("routes.yaml: {0}")]
    Routes(#[from] RoutesError),
    #[error("services.yaml: {0}")]
    Services(#[from] ServicesError),
    #[error("datasource_types.yaml: {0}")]
    DatasourceTypes(#[from] DatasourceTypesError),
    #[error("settings.yaml: {0}")]
    Settings(#[from] SettingsConfigError),
    #[error("view_types.yaml: {0}")]
    ViewTypes(#[from] ViewTypesError),
    #[error("backend-app.yaml: {0}")]
    BackendAppConfig(#[from] BackendAppConfigError),
    #[error("middleware lookup: {0}")]
    Lookup(#[from] LookupError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct BuiltApp {
    pub app_name: String,
    pub router: Router,
    pub datasource: Arc<OpenDatasource>,
    pub registry: ServiceRegistry,
    pub registered_routes: Vec<String>,
    pub skipped_routes: Vec<String>,
}

#[derive(Default)]
pub struct TraceWriters {
    pub route: Option<Box<dyn Write + Send>>,
    pub service: Option<Box<dyn Write + Send>>,
    pub datasource: Option<Box<dyn Write + Send>>,
}

impl TraceWriters {
    pub fn route_only(writer: Box<dyn Write + Send>) -> Self {
        Self {
            route: Some(writer),
            service: None,
            datasource: None,
        }
    }
}

pub async fn build_app(config: &RunConfig) -> Result<BuiltApp, RunError> {
    build_app_with_trace_writers(config, TraceWriters::default()).await
}

pub async fn build_app_with_trace_writer(
    config: &RunConfig,
    trace_writer_override: Option<Box<dyn Write + Send>>,
) -> Result<BuiltApp, RunError> {
    let writers = match trace_writer_override {
        Some(w) => TraceWriters::route_only(w),
        None => TraceWriters::default(),
    };
    build_app_with_trace_writers(config, writers).await
}

pub async fn build_app_with_trace_writers(
    config: &RunConfig,
    writers: TraceWriters,
) -> Result<BuiltApp, RunError> {
    let det_dir: &Path = &config.deterministic_dir;
    if !det_dir.exists() {
        return Err(RunError::Repository(RepositoryError::Other(format!(
            "deterministic dir not found: {} (set DETERMINISTIC_DIR to the directory holding datasource_types.yaml)",
            det_dir.display()
        ))));
    }
    let settings = load_settings_config(&det_dir.join("settings.yaml"))?;
    let ds_doc = load_datasource_types(&det_dir.join("datasource_types.yaml"))?;
    let entities = ds_doc.types;
    let routes_doc = load_routes(&det_dir.join("routes.yaml"))?;
    let services_specs = load_services(&det_dir.join("services.yaml"))?;
    let _view_doc = load_view_types(&det_dir.join("view_types.yaml"))?;

    let TraceWriters {
        route: route_writer,
        service: service_writer,
        datasource: datasource_writer,
    } = writers;

    let backend_config = load_backend_app_config_or_default(det_dir)?;
    let app_name = settings
        .application_name
        .clone()
        .unwrap_or_else(|| backend_config.name.clone());
    let middleware_entries = resolve_middleware_entries(backend_config.middleware.clone());

    let mut datasource_lookup = DataSourceMiddlewareLookup::empty();
    if let Some(w) = datasource_writer {
        datasource_lookup = datasource_lookup.with_trace_writer(w);
    }
    let _datasource_chain = build_datasource_chain(&middleware_entries, &datasource_lookup)?;

    let mut service_lookup = ServiceMiddlewareLookup::empty();
    if let Some(w) = service_writer {
        service_lookup = service_lookup.with_trace_writer(w);
    }
    let service_chain = build_service_chain(&middleware_entries, &service_lookup)?;

    let datasource = Arc::new(open_datasource(&config.database_url).await?);

    let (registry, raw_registry) = build_service_registry(
        &entities,
        &services_specs,
        &config.custom_services,
        &service_chain,
    )?;

    let composed_entity_router = match config.route_composer.as_ref() {
        Some(composer) => {
            let ctx = ComposeContext::new(registry.clone());
            Some(composer.compose(&ctx).map_err(RunError::Repository)?)
        }
        None => None,
    };

    let combined_descriptors =
        super::combined_routes::expand_combined_routes(&routes_doc.combined, &entities)
            .map_err(|e| RunError::Repository(RepositoryError::Other(e.to_string())))?;
    let (mut router, report) = build_router(&routes_doc.generic, &registry);
    if let Some(entity_router) = composed_entity_router {
        router = router.merge(entity_router);
    }
    let router = super::combined_routes::mount_combined_routes(
        router,
        &combined_descriptors,
        &registry,
        &raw_registry,
    );
    let health_handler_registered = report.registered_paths.iter().any(|p| p == "/api/health");
    let router = if health_handler_registered {
        router
    } else {
        router.merge(crate::app::health::router())
    };
    let router = mount_statics(router, backend_config.statics.as_deref());
    let router = if is_handler_enabled(&backend_config, "NotFoundMiddlewareService") {
        router.fallback(crate::middleware::not_found_fallback)
    } else {
        router
    };
    let router = if is_handler_enabled(&backend_config, "ErrorHandlerMiddlewareService") {
        router
            .layer(axum::middleware::from_fn(
                crate::middleware::envelope_error_layer,
            ))
            .layer(tower_http::catch_panic::CatchPanicLayer::custom(
                crate::middleware::EnvelopePanicResponder,
            ))
    } else {
        router
    };
    let trace_enabled = route_writer.is_some() || trace_route_enabled_from_env();
    let router = if trace_enabled {
        let middleware = match route_writer {
            Some(w) => TraceRouteMiddleware::with_writer(w),
            None => TraceRouteMiddleware::new(),
        };
        let logger = Arc::new(middleware);
        router.layer(axum::middleware::from_fn_with_state(
            logger,
            trace_route_layer,
        ))
    } else {
        router
    };
    Ok(BuiltApp {
        app_name,
        router,
        datasource,
        registry,
        registered_routes: report.registered,
        skipped_routes: report.skipped_missing_service,
    })
}

fn build_service_registry(
    entities: &[crate::loaders::DatasourceTypeDef],
    services_specs: &[ServiceSpec],
    custom_services: &CustomServices,
    service_chain: &[Arc<dyn ServiceMiddleware>],
) -> Result<(ServiceRegistry, ServiceRegistry), RunError> {
    let mut builder = ServiceRegistryBuilder::new();
    for (name, service) in custom_services.iter() {
        let wrapped: Arc<dyn DynamicService> =
            ServiceWithMiddleware::wrap(service.clone(), service_chain.to_vec(), name.clone());
        builder = builder.register(name, wrapped);
    }
    let custom_service_names: std::collections::HashSet<&str> = custom_services.names().collect();
    let entity_service_names: std::collections::HashSet<&str> =
        entities.iter().map(|e| e.name.as_str()).collect();

    for spec in services_specs {
        if entity_service_names.contains(spec.name.as_str()) {
            continue;
        }
        if custom_service_names.contains(spec.name.as_str()) {
            continue;
        }
        return Err(RunError::Repository(RepositoryError::Other(format!(
            "services.yaml declares service \"{}\" (module: {:?}) but the Rust runtime has no \
             implementation registered for it. Either (a) remove the entry from services.yaml, \
             (b) provide a generated Datasources-backed service and register it on \
             RunConfig::custom_services in main.rs, or (c) use the TypeScript backend for this \
             consumer.",
            spec.name, spec.module,
        ))));
    }
    let registry = builder.build();
    Ok((registry.clone(), registry))
}

fn load_backend_app_config_or_default(
    det_dir: &Path,
) -> Result<BackendAppConfig, BackendAppConfigError> {
    let path = det_dir.join("backend-app.yaml");
    if path.exists() {
        load_backend_app_config(&path)
    } else {
        Ok(BackendAppConfig {
            name: crate::backend_app_config::DEFAULT_APP_NAME.to_string(),
            middleware: crate::backend_app_config::DEFAULT_MIDDLEWARE
                .iter()
                .map(|name| crate::backend_app_config::MiddlewareEntry {
                    name: name.to_string(),
                    r#type: "app".to_string(),
                    enabled: true,
                    apply_routes: None,
                    deny_routes: None,
                })
                .collect(),
            handlers: crate::backend_app_config::DEFAULT_HANDLERS
                .iter()
                .map(|name| crate::backend_app_config::HandlerEntry {
                    name: name.to_string(),
                    enabled: true,
                })
                .collect(),
            statics: None,
        })
    }
}

fn mount_statics(
    mut router: Router,
    statics: Option<&[crate::backend_app_config::StaticEntry]>,
) -> Router {
    let Some(entries) = statics else {
        return router;
    };
    for entry in entries {
        router = router.nest_service(&entry.path, tower_http::services::ServeDir::new(&entry.dir));
    }
    router
}

fn resolve_middleware_entries(mut entries: Vec<MiddlewareEntry>) -> Vec<MiddlewareEntry> {
    let env_value = std::env::var("DETERMINISTIC_TRACE").ok();
    let enable = parse_enable_middleware_env(env_value.as_deref());
    apply_enable_middleware(&mut entries, &enable);
    entries
}

fn build_datasource_chain(
    entries: &[MiddlewareEntry],
    lookup: &DataSourceMiddlewareLookup,
) -> Result<Vec<Arc<dyn DataSourceMiddleware>>, LookupError> {
    let mut chain = Vec::new();
    for entry in entries {
        if entry.r#type == "datasource" && entry.enabled {
            chain.push(lookup.get(&entry.name)?);
        }
    }
    Ok(chain)
}

fn build_service_chain(
    entries: &[MiddlewareEntry],
    lookup: &ServiceMiddlewareLookup,
) -> Result<Vec<Arc<dyn ServiceMiddleware>>, LookupError> {
    let mut chain = Vec::new();
    for entry in entries {
        if entry.r#type == "service" && entry.enabled {
            chain.push(lookup.get(&entry.name)?);
        }
    }
    Ok(chain)
}

pub async fn run(config: RunConfig) -> Result<(), RunError> {
    let handle = crate::app::serve::start(config).await?;
    crate::app::serve::wait_for_shutdown_signal().await;
    println!(
        "\n{} received shutdown signal, shutting down.",
        handle.app_name
    );
    handle.close().await
}

pub async fn serve(app: BuiltApp, listener: TcpListener) -> Result<(), RunError> {
    axum::serve(listener, app.router).await?;
    Ok(())
}

fn trace_route_enabled_from_env() -> bool {
    let Ok(val) = std::env::var("DETERMINISTIC_TRACE") else {
        return false;
    };
    val.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .any(|t| t == "route" || t == "traceroute")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_route_enabled_from_env_parses_route_tier() {
        let prev = std::env::var("DETERMINISTIC_TRACE").ok();
        std::env::set_var("DETERMINISTIC_TRACE", "route,service,datasource");
        assert!(trace_route_enabled_from_env());
        std::env::set_var("DETERMINISTIC_TRACE", "service,datasource");
        assert!(!trace_route_enabled_from_env());
        std::env::set_var("DETERMINISTIC_TRACE", " Route ");
        assert!(trace_route_enabled_from_env());
        std::env::remove_var("DETERMINISTIC_TRACE");
        assert!(!trace_route_enabled_from_env());
        if let Some(v) = prev {
            std::env::set_var("DETERMINISTIC_TRACE", v);
        }
    }
}
