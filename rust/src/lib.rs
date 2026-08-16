pub mod app;
pub mod backend_app_config;
pub mod error;
pub mod id_type;
pub mod loaders;
pub mod mappings;
pub mod middleware;
pub mod repositories;
pub mod router;
pub mod routes;
pub mod run;
pub mod services;
pub mod sql_identifier;
pub mod trace;
pub mod types;
pub(crate) mod util;

pub use backend_app_config::{
    apply_enable_middleware, is_handler_enabled, is_middleware_enabled, load_backend_app_config,
    parse_enable_middleware_env, BackendAppConfig, BackendAppConfigError, HandlerEntry,
    MiddlewareEntry, StaticEntry, DEFAULT_HANDLERS, DEFAULT_MIDDLEWARE,
};
pub use error::RepositoryError;
pub use id_type::IdType;
pub use mappings::{
    get_entity_field_map, parse_field_mappings, EntityFieldMap, FieldMappingTranslator,
    FieldMappings, TypeFieldConverter,
};
pub use middleware::{
    trace_route_layer, DataSourceConsoleLoggerMiddleware, ServiceConsoleLoggerMiddleware,
    TraceRouteMiddleware,
};
pub use services::{
    AuthError, AuthenticationService, AuthorizationService, CanAccessPermissionResult,
    CustomServices, DynamicService, GenericCrudService, ServiceError, UserInfoResult,
};
pub use sql_identifier::{quote_identifier, validate_identifier};
pub use trace::{format_elapsed_ms, format_trace_line, TracePhase, TraceTier};
pub use types::StandardTable;

pub use repositories::{
    run_query_with_middlewares, CrudRepository, DataSourceMiddleware, DatabaseBackend, Datasource,
    Repository, RewrittenQuery, RowMap, Setup, StandardCrudRepository,
};

pub use router::{build_router, RouterBuildReport, ServiceRegistry, ServiceRegistryBuilder};

pub use app::serve::{start, wait_for_shutdown_signal, StartHandle};
pub use run::{
    build_app, build_app_with_trace_writer, build_app_with_trace_writers, run, serve, BuiltApp,
    ComposeContext, RouteComposer, RunConfig, RunError, TraceWriters,
};
