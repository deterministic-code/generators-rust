//! Oracle backend.
//!
//! There is no production-grade async-native Oracle driver in the Rust
//! ecosystem (the `oracle` crate is sync-only and depends on the Oracle
//! Instant Client). This module exposes the same trait surface as every
//! other backend so generated services can target Oracle without
//! conditionally compiling — but every method returns
//! [`RepositoryError::Unimplemented`]. SQL-string generation works via
//! the shared [`crate::repositories::sql_builder`] helpers.

pub mod oracle_crud_repository;
pub mod oracle_datasource;
pub mod oracle_repository;
pub mod oracle_setup;
pub mod oracle_standard_repository;

pub use oracle_crud_repository::OracleCrudRepository;
pub use oracle_datasource::{OracleDatasource, OracleDatasourceOptions};
pub use oracle_repository::OracleRepository;
pub use oracle_setup::{OracleSetup, OracleSetupOptions};
pub use oracle_standard_repository::OracleStandardRepository;
