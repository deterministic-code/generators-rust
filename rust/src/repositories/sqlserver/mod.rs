//! SQL Server backend.
//!
//! This module currently provides SQL-string generation only. A live driver
//! (`tiberius` + `bb8-tiberius`) will be wired in a follow-up — every runtime
//! method returns [`RepositoryError::Unimplemented`]. The `build_*_sql`
//! methods on [`SqlserverCrudRepository`] are pure functions and fully tested.

pub mod sqlserver_crud_repository;
pub mod sqlserver_datasource;
pub mod sqlserver_repository;
pub mod sqlserver_setup;
pub mod sqlserver_standard_repository;

pub use sqlserver_crud_repository::SqlserverCrudRepository;
pub use sqlserver_datasource::{SqlserverDatasource, SqlserverDatasourceOptions};
pub use sqlserver_repository::SqlserverRepository;
pub use sqlserver_setup::{SqlserverSetup, SqlserverSetupOptions};
pub use sqlserver_standard_repository::SqlserverStandardRepository;
