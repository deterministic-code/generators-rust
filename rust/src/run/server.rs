use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use thiserror::Error;
use tokio::net::TcpListener;

use std::collections::{BTreeMap, HashMap};

use crate::backend_app_config::{
    apply_enable_middleware, is_handler_enabled, load_backend_app_config,
    parse_enable_middleware_env, BackendAppConfig, BackendAppConfigError, MiddlewareEntry,
};
use crate::error::RepositoryError;
use crate::id_type::IdType;
use crate::loaders::{
    build_eager_read_bindings, build_eager_write_bindings, build_entities_by_name,
    compute_enrichments_for_entity, load_datasource_types, load_routes, load_services,
    load_settings_config, load_view_types, BindingKind, DatasourceMapping, DatasourceTypeDef,
    DatasourceTypesError, EagerWriteChildBinding, RoutesError, ServiceSpec, ServicesError,
    SettingsConfig, SettingsConfigError, ViewTypesError,
};
use crate::mappings::parse_field_mappings::EntityFieldMap;
use crate::middleware::{
    trace_route_layer, DataSourceMiddlewareLookup, LookupError, ServiceMiddlewareLookup,
    TraceRouteMiddleware,
};
use crate::repositories::datasource_middleware::DataSourceMiddleware;
use crate::repositories::Datasource;
use crate::services::{
    CrudRepoFactory, CustomServices, DynamicService, EagerChildReadingService,
    EagerChildWritingService, EagerWriteChildBindingRuntime, GenericCrudService,
    LookupEnrichedService, ServiceFactory, ServiceMiddleware, ServiceWithMiddleware,
};
use crate::{build_router, ServiceRegistry, ServiceRegistryBuilder};

use super::compose::ComposeContext;
use super::config::RunConfig;
use super::datasource::{
    build_crud_repo_for_datasource, open_datasource, DialectKind, OpenDatasource,
};

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
    // A missing directory is a misconfigured DETERMINISTIC_DIR, not an empty project: the per-file
    // loaders below tolerate an absent yaml (a project with no datasource types is valid), so without
    // this guard a wrong path silently yields zero entities — and the route composer then fails with a
    // baffling "no service registered for entity X" instead of the real cause.
    if !det_dir.exists() {
        return Err(RunError::Repository(RepositoryError::Other(format!(
            "deterministic dir not found: {} (set DETERMINISTIC_DIR to the directory holding datasource_types.yaml)",
            det_dir.display()
        ))));
    }
    let settings = load_settings_config(&det_dir.join("settings.yaml"))?;
    let ds_doc = load_datasource_types(&det_dir.join("datasource_types.yaml"))?;
    let entities = ds_doc.types;
    let datasource_mappings = ds_doc.datasource_mappings;
    let routes_doc = load_routes(&det_dir.join("routes.yaml"))?;
    let services_specs = load_services(&det_dir.join("services.yaml"))?;
    let view_doc = load_view_types(&det_dir.join("view_types.yaml"))?;
    let auto_enrich = view_doc.datasource_inclusion.auto_enrich;
    let eager_write_bindings = build_eager_write_bindings(&routes_doc, &view_doc, &entities);
    let eager_read_bindings = build_eager_read_bindings(&routes_doc, &view_doc, &entities);

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
    let datasource_chain = build_datasource_chain(&middleware_entries, &datasource_lookup)?;

    let mut service_lookup = ServiceMiddlewareLookup::empty();
    if let Some(w) = service_writer {
        service_lookup = service_lookup.with_trace_writer(w);
    }
    let service_chain = build_service_chain(&middleware_entries, &service_lookup)?;

    let datasource = Arc::new(open_datasource(&config.database_url).await?);

    let (registry, raw_registry) = build_service_registry(
        &entities,
        &datasource_mappings,
        &datasource,
        &settings,
        auto_enrich,
        &datasource_chain,
        &service_chain,
        &eager_write_bindings,
        &eager_read_bindings,
        &services_specs,
        &config.custom_services,
    )?;

    // The generated backend's route composer owns every per-entity route (CRUD + by-field), routing
    // to the generated services. Without one, only the declared generic routes (health, custom
    // services) plus the combined m2m sub-routes below are mounted — the dynamic CRUD/by-field
    // synthesis is gone.
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
    // Mount the builtin /api/health only when no real handler registered (see PR #1038).
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
    entities: &[DatasourceTypeDef],
    datasource_mappings: &[DatasourceMapping],
    datasource: &Arc<OpenDatasource>,
    settings: &SettingsConfig,
    auto_enrich: bool,
    datasource_chain: &[Arc<dyn DataSourceMiddleware>],
    service_chain: &[Arc<dyn ServiceMiddleware>],
    eager_write_bindings: &HashMap<String, Vec<EagerWriteChildBinding>>,
    eager_read_bindings: &HashMap<String, Vec<EagerWriteChildBinding>>,
    services_specs: &[ServiceSpec],
    custom_services: &CustomServices,
) -> Result<(ServiceRegistry, ServiceRegistry), RunError> {
    let mut base_services: HashMap<String, Arc<dyn DynamicService>> = HashMap::new();
    for entity in entities {
        if entity.target.as_deref() == Some("None") {
            continue;
        }
        let (table, primary_key, entity_map, column_datasource_types) =
            resolve_entity_persistence(entity, datasource_mappings, settings);
        let repo = datasource.build_crud_repo_with_mapping(
            &table,
            &primary_key,
            entity_map.as_ref(),
            datasource_chain,
            &column_datasource_types,
            entity_id_type(entity),
        )?;
        let use_occ =
            entity.uses_optimistic_concurrency(settings.datasource.use_optimistic_concurrency);
        let service: Arc<dyn DynamicService> =
            Arc::new(GenericCrudService::new(repo).with_optimistic_concurrency(use_occ));
        base_services.insert(entity.name.clone(), service);
    }

    // The combined m2m route handlers filter junction rows by their raw FK columns; enrichment
    // (replace_fk) strips those, so the join must read the pre-enrichment services.
    let raw_registry = ServiceRegistry::from_map(base_services.clone());

    let entities_map = build_entities_by_name(entities);
    let dialect = datasource.dialect();
    let mut builder = ServiceRegistryBuilder::new();
    for entity in entities {
        let Some(base) = base_services.get(&entity.name).cloned() else {
            continue;
        };
        let mut enrichments = compute_enrichments_for_entity(entity, &entities_map);
        for e in enrichments.iter_mut() {
            e.replace_fk = auto_enrich;
        }
        let mut pool_lookups: HashMap<String, Arc<dyn DynamicService>> = HashMap::new();
        let mut lookup_factories: HashMap<String, ServiceFactory> = HashMap::new();
        for e in &enrichments {
            if let Some(svc) = base_services.get(&e.target_table) {
                pool_lookups.insert(e.target_table.clone(), svc.clone());
            }
            if let Some(target_entity) = entities.iter().find(|x| x.name == e.target_table) {
                lookup_factories.insert(
                    e.target_table.clone(),
                    make_service_factory(
                        target_entity,
                        datasource_mappings,
                        settings,
                        &dialect,
                        datasource_chain,
                        Vec::new(),
                        HashMap::new(),
                    ),
                );
            }
        }
        let enriched: Arc<dyn DynamicService> = if enrichments.is_empty() {
            base.clone()
        } else {
            Arc::new(LookupEnrichedService::new(
                base.clone(),
                enrichments.clone(),
                pool_lookups,
            ))
        };

        let wrapped: Arc<dyn DynamicService> = match eager_write_bindings.get(&entity.name) {
            Some(bindings) if !bindings.is_empty() => {
                let runtime_bindings = build_runtime_bindings(
                    bindings,
                    entities,
                    datasource_mappings,
                    settings,
                    &dialect,
                    datasource_chain,
                    auto_enrich,
                )?;
                let (_, parent_pk, _, _) =
                    resolve_entity_persistence(entity, datasource_mappings, settings);
                let parent_factory = make_service_factory(
                    entity,
                    datasource_mappings,
                    settings,
                    &dialect,
                    datasource_chain,
                    enrichments.clone(),
                    lookup_factories,
                );
                Arc::new(EagerChildWritingService::new(
                    enriched,
                    datasource.as_datasource(),
                    parent_factory,
                    parent_pk,
                    runtime_bindings,
                ))
            }
            _ => enriched,
        };

        // Eager READ wraps outermost: on find/find_all it attaches each `eager_path` relation to the
        // already-enriched parent row (the analog of TS's EagerChildLoadingService decorator).
        let read_wrapped: Arc<dyn DynamicService> = match eager_read_bindings.get(&entity.name) {
            Some(bindings) if !bindings.is_empty() => {
                let runtime_bindings = build_runtime_bindings(
                    bindings,
                    entities,
                    datasource_mappings,
                    settings,
                    &dialect,
                    datasource_chain,
                    auto_enrich,
                )?;
                let (_, parent_pk, _, _) =
                    resolve_entity_persistence(entity, datasource_mappings, settings);
                Arc::new(EagerChildReadingService::new(
                    wrapped,
                    datasource.as_datasource(),
                    parent_pk,
                    runtime_bindings,
                ))
            }
            _ => wrapped,
        };

        let service =
            ServiceWithMiddleware::wrap(read_wrapped, service_chain.to_vec(), entity.name.clone());
        builder = builder.register(&entity.name, service);
    }

    // Custom Rust impls (main.rs registers them on RunConfig::custom_services) preempt the hard-fail below.
    for (name, service) in custom_services.iter() {
        builder = builder.register(name, service.clone());
    }
    let custom_service_names: std::collections::HashSet<&str> = custom_services.names().collect();

    let entity_service_names: std::collections::HashSet<&str> =
        entities.iter().map(|e| e.name.as_str()).collect();
    // Hard-fail (PR #1041) for any declared service with no entity, no custom Rust impl, and no fallback stub.
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
             (b) add the service to entities so an auto-generated CRUD service is registered, \
             (c) provide a Rust impl at src/services/custom/<snake>.rs and register it on \
             RunConfig::custom_services in main.rs (re-run `create-services --language Rust` \
             to scaffold both), or (d) use the TypeScript backend for this consumer. Silently \
             registering a null stub would mask the missing implementation as a 200/null \
             response downstream.",
            spec.name, spec.module,
        ))));
    }
    Ok((builder.build(), raw_registry))
}

pub(crate) fn resolve_entity_persistence(
    entity: &DatasourceTypeDef,
    datasource_mappings: &[DatasourceMapping],
    settings: &SettingsConfig,
) -> (
    String,
    String,
    Option<EntityFieldMap>,
    BTreeMap<String, String>,
) {
    let mapping = datasource_mappings.iter().find(|m| m.entity == entity.name);
    let table = match mapping {
        Some(m) if !m.source.is_empty() => m.source.clone(),
        _ if settings.datasource.pluralize_table_names => {
            super::crud_routes::pluralize_table_name(&entity.name)
        }
        _ => entity.name.clone(),
    };
    let primary_key = entity
        .fields
        .iter()
        .find(|f| f.primary_key)
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "id".to_string());
    let entity_map = mapping.map(entity_field_map_from_mapping);
    // why logical keys: converter dispatch happens after to_logical_row, so the map is keyed on the YAML field name, not the renamed physical column.
    // why resolve `reference`: a typeless FK whose parent has no explicit PK stays the `reference`
    // sentinel (the loader can only inherit an explicit parent PK type) — implicit `id` is integer.
    let id_type = entity_id_type(entity);
    let mut column_datasource_types: BTreeMap<String, String> = entity
        .fields
        .iter()
        .map(|f| {
            let ds_type = if f.r#type == "reference" {
                "number".to_string()
            } else {
                f.r#type.clone()
            };
            (f.name.clone(), ds_type)
        })
        .collect();
    // A client-supplied id has no declared field, so register the synthetic primary key with the
    // PK field type — otherwise its bind gets no converter cast and postgres rejects text→uuid
    // on insert. DB-assigned ids are never bound, so the owner keeps them out.
    if id_type.is_client_supplied() {
        column_datasource_types
            .entry(primary_key.clone())
            .or_insert_with(|| id_type.datasource_type_str().to_string());
    }
    (table, primary_key, entity_map, column_datasource_types)
}

fn entity_id_type(entity: &DatasourceTypeDef) -> IdType {
    entity
        .fields
        .iter()
        .find(|f| f.primary_key)
        .or_else(|| entity.fields.iter().find(|f| f.name == "id"))
        .map(|f| IdType::from_field_type(&f.r#type))
        .unwrap_or_default()
}

fn make_service_factory(
    entity: &DatasourceTypeDef,
    datasource_mappings: &[DatasourceMapping],
    settings: &SettingsConfig,
    dialect: &DialectKind,
    datasource_chain: &[Arc<dyn DataSourceMiddleware>],
    enrichments: Vec<crate::loaders::EnrichmentSpec>,
    lookup_factories: HashMap<String, ServiceFactory>,
) -> ServiceFactory {
    let (table, primary_key, entity_map, column_datasource_types) =
        resolve_entity_persistence(entity, datasource_mappings, settings);
    let dialect = dialect.clone();
    let middlewares = datasource_chain.to_vec();
    let id_type = entity_id_type(entity);
    let use_occ =
        entity.uses_optimistic_concurrency(settings.datasource.use_optimistic_concurrency);
    Arc::new(move |txn_ds: Arc<dyn Datasource>| {
        let repo = build_crud_repo_for_datasource(
            dialect.clone(),
            txn_ds.clone(),
            &table,
            &primary_key,
            entity_map.as_ref(),
            &middlewares,
            &column_datasource_types,
            id_type,
        )?;
        let mut svc: Arc<dyn DynamicService> =
            Arc::new(GenericCrudService::new(repo).with_optimistic_concurrency(use_occ));
        if !enrichments.is_empty() {
            let mut txn_lookups: HashMap<String, Arc<dyn DynamicService>> = HashMap::new();
            for (k, f) in &lookup_factories {
                txn_lookups.insert(k.clone(), f(txn_ds.clone())?);
            }
            svc = Arc::new(LookupEnrichedService::new(
                svc,
                enrichments.clone(),
                txn_lookups,
            ));
        }
        Ok(svc)
    })
}

fn make_junction_repo_factory(
    table: String,
    dialect: DialectKind,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
    id_type: IdType,
) -> CrudRepoFactory {
    Arc::new(move |txn_ds: Arc<dyn Datasource>| {
        build_crud_repo_for_datasource(
            dialect.clone(),
            txn_ds,
            &table,
            "id",
            None,
            &middlewares,
            &BTreeMap::new(),
            id_type,
        )
    })
}

fn build_runtime_bindings(
    bindings: &[EagerWriteChildBinding],
    entities: &[DatasourceTypeDef],
    datasource_mappings: &[DatasourceMapping],
    settings: &SettingsConfig,
    dialect: &DialectKind,
    datasource_chain: &[Arc<dyn DataSourceMiddleware>],
    auto_enrich: bool,
) -> Result<Vec<EagerWriteChildBindingRuntime>, RunError> {
    let entities_map = build_entities_by_name(entities);
    let mut out = Vec::with_capacity(bindings.len());
    for b in bindings {
        let Some(child_entity) = entities.iter().find(|e| e.name == b.child_table) else {
            return Err(RunError::Repository(RepositoryError::Other(format!(
                "eager-write binding references unknown entity `{}`",
                b.child_table
            ))));
        };
        let mut child_enrichments = compute_enrichments_for_entity(child_entity, &entities_map);
        for e in child_enrichments.iter_mut() {
            e.replace_fk = auto_enrich;
        }
        let mut child_lookup_factories: HashMap<String, ServiceFactory> = HashMap::new();
        for e in &child_enrichments {
            if let Some(target_entity) = entities.iter().find(|x| x.name == e.target_table) {
                child_lookup_factories.insert(
                    e.target_table.clone(),
                    make_service_factory(
                        target_entity,
                        datasource_mappings,
                        settings,
                        dialect,
                        datasource_chain,
                        Vec::new(),
                        HashMap::new(),
                    ),
                );
            }
        }
        let child_service_factory = make_service_factory(
            child_entity,
            datasource_mappings,
            settings,
            dialect,
            datasource_chain,
            child_enrichments,
            child_lookup_factories,
        );
        let junction_repo_factory = match &b.kind {
            BindingKind::M2m { junction_table, .. } => {
                let physical_table = datasource_mappings
                    .iter()
                    .find(|m| &m.entity == junction_table && !m.source.is_empty())
                    .map(|m| m.source.clone())
                    .unwrap_or_else(|| {
                        if settings.datasource.pluralize_table_names {
                            super::crud_routes::pluralize_table_name(junction_table)
                        } else {
                            junction_table.clone()
                        }
                    });
                Some(make_junction_repo_factory(
                    physical_table,
                    dialect.clone(),
                    datasource_chain.to_vec(),
                    IdType::Integer,
                ))
            }
            BindingKind::DirectFk { .. } => None,
        };
        let nested = build_runtime_bindings(
            &b.children,
            entities,
            datasource_mappings,
            settings,
            dialect,
            datasource_chain,
            auto_enrich,
        )?;
        out.push(EagerWriteChildBindingRuntime {
            binding: b.clone(),
            child_service_factory,
            junction_repo_factory,
            children: nested,
        });
    }
    Ok(out)
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

fn entity_field_map_from_mapping(m: &DatasourceMapping) -> EntityFieldMap {
    let mut out = EntityFieldMap::new();
    for fm in &m.field_mappings {
        out.insert(fm.field.clone(), fm.source.clone());
    }
    out
}

/// Mirrors typescript/src/app/start.ts's default flow: start, wait for SIGINT/SIGTERM, then drain and close.
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

// Mirrors typescript/src/app/enableMiddleware.ts::parseEnableMiddlewareEnv — the `route` tier shortcut enables the trace_route layer.
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
    use crate::loaders::datasource_types::FieldDef;

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

    fn member_like_entity() -> DatasourceTypeDef {
        DatasourceTypeDef {
            name: "member".to_string(),
            fields: vec![FieldDef {
                name: "handle".to_string(),
                r#type: "string".to_string(),
                is_unique: true,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn persistence_for(entity: &DatasourceTypeDef) -> BTreeMap<String, String> {
        let settings = crate::loaders::settings::parse_settings_config(
            "settings:\n  datasource: {}\n",
        )
        .unwrap();
        let (_table, _pk, _map, column_types) =
            resolve_entity_persistence(entity, &[], &settings);
        column_types
    }

    fn entity_with_id(id_type: &str) -> DatasourceTypeDef {
        let mut entity = member_like_entity();
        entity.fields.insert(
            0,
            FieldDef {
                name: "id".to_string(),
                r#type: id_type.to_string(),
                primary_key: true,
                ..Default::default()
            },
        );
        entity
    }

    #[test]
    fn authored_uuid_pk_registers_uuid_column_type_so_postgres_gets_its_cast() {
        assert_eq!(
            persistence_for(&entity_with_id("uuid"))
                .get("id")
                .map(String::as_str),
            Some("uuid"),
        );
    }

    #[test]
    fn authored_string_pk_registers_string_column_type() {
        assert_eq!(
            persistence_for(&entity_with_id("string"))
                .get("id")
                .map(String::as_str),
            Some("string"),
        );
    }

    #[test]
    fn implicit_integer_pk_stays_unregistered_db_assigns_it() {
        assert!(!persistence_for(&member_like_entity()).contains_key("id"));
    }

    fn conversation_like_entity() -> DatasourceTypeDef {
        DatasourceTypeDef {
            name: "conversation".to_string(),
            fields: vec![FieldDef {
                name: "created_by".to_string(),
                r#type: "reference".to_string(),
                references: Some("member.id".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn typeless_fk_to_implicit_id_carries_number() {
        let settings = crate::loaders::settings::parse_settings_config(
            "settings:\n  datasource: {}\n",
        )
        .unwrap();
        let (_table, _pk, _map, column_types) =
            resolve_entity_persistence(&conversation_like_entity(), &[], &settings);
        assert_eq!(column_types.get("created_by").map(String::as_str), Some("number"));
    }
}
