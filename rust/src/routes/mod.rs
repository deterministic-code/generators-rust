pub mod adapt;
pub mod by_field;
pub mod combined;
pub mod crud;
pub mod generic;
pub(crate) mod helpers;
pub mod nested_crud;
pub mod nested_many_to_many;
pub mod read_only;

pub use by_field::{create_by_field_router, ByFieldMethod, ByFieldRouterConfig, ByFieldService};
pub use combined::{iterate_combined_routers, mount_combined_routes};
pub use crud::{
    create_crud_router, CoerceRowFn, CrudRouterConfig, CrudService, EnrichItemFn, EnrichItemsFn,
    EnrichmentFuture, ResolveItemFn, RouteHookError, ValidatorFn,
};
pub use generic::{
    create_generic_router, GenericHandlerFn, GenericHandlerFuture, GenericResponseFormat,
    GenericRouterConfig, GenericRouterMethod,
};
pub use helpers::IdType;
pub use nested_crud::{create_nested_crud_router, NestedCrudRouterConfig};
pub use nested_many_to_many::{
    create_nested_many_to_many_router, LinkVerb, NestedManyToManyRouterConfig,
};
pub use read_only::{create_read_only_router, ReadOnlyRouterConfig, ReadOnlyService};
