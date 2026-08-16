pub mod combined_routes;
pub mod compose;
pub mod config;
pub mod converter_defaults;
pub mod crud_routes;
pub mod datasource;
pub mod server;

pub use combined_routes::{
    expand_combined_routes, mount_combined_routes, CombinedDescriptor, CombinedRoutesError,
    DirectFkDescriptor, M2mDescriptor,
};
pub use compose::{ComposeContext, RouteComposer};
pub use config::RunConfig;
pub use crud_routes::{pluralize_path_segment, pluralize_table_name};
pub use datasource::{open_datasource, DialectKind, OpenDatasource};
pub use server::{
    build_app, build_app_with_trace_writer, build_app_with_trace_writers, run, serve, BuiltApp,
    RunError, TraceWriters,
};
