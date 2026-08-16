use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::crud_repository::CrudRepository;
use crate::repositories::datasource_middleware::run_query_with_middlewares;
use crate::repositories::datasource_middleware::{DataSourceMiddleware, DatabaseBackend};
use crate::repositories::oracle::oracle_datasource::OracleDatasource;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::repositories::sql_builder::{
    build_delete, build_insert, build_select_all, build_select_by_column, build_select_by_id,
    build_update, Dialect,
};

const DIALECT: Dialect = Dialect::Oracle;
const BACKEND: DatabaseBackend = DatabaseBackend::Oracle;
const UNAVAILABLE: &str = "oracle backend not available in Rust ecosystem";

pub struct OracleCrudRepository {
    datasource: Arc<OracleDatasource>,
    table_name: String,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
}

impl OracleCrudRepository {
    pub fn new(
        datasource: Arc<OracleDatasource>,
        table_name: impl Into<String>,
    ) -> Result<Self, RepositoryError> {
        let table_name = table_name.into();
        DIALECT.quote(&table_name)?;
        Ok(Self {
            datasource,
            table_name,
            middlewares: Vec::new(),
        })
    }

    pub fn with_middlewares(mut self, middlewares: Vec<Arc<dyn DataSourceMiddleware>>) -> Self {
        self.middlewares = middlewares;
        self
    }

    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    pub fn build_find_sql(&self) -> Result<String, RepositoryError> {
        build_select_by_id(DIALECT, &self.table_name, "id")
    }
    pub fn build_find_all_sql(&self) -> Result<String, RepositoryError> {
        build_select_all(DIALECT, &self.table_name, "id")
    }
    pub fn build_find_by_sql(&self, column: &str) -> Result<String, RepositoryError> {
        build_select_by_column(DIALECT, &self.table_name, column, "id")
    }
    pub fn build_add_sql(&self, columns: &[&str]) -> Result<String, RepositoryError> {
        build_insert(DIALECT, &self.table_name, columns, "id")
    }
    pub fn build_update_sql(&self, columns: &[&str]) -> Result<String, RepositoryError> {
        build_update(DIALECT, &self.table_name, columns, "id")
    }
    pub fn build_delete_sql(&self) -> Result<String, RepositoryError> {
        build_delete(DIALECT, &self.table_name, "id")
    }
}

#[async_trait]
impl Repository for OracleCrudRepository {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        run_query_with_middlewares(
            &self.middlewares,
            BACKEND,
            self.datasource.as_ref(),
            sql,
            params,
        )
        .await
    }
}

#[async_trait]
impl CrudRepository for OracleCrudRepository {
    async fn find(&self, _id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        Err(RepositoryError::Unimplemented(UNAVAILABLE))
    }
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::Unimplemented(UNAVAILABLE))
    }
    async fn find_by(&self, _column: &str, _value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::Unimplemented(UNAVAILABLE))
    }
    async fn add(&self, _data: RowMap) -> Result<RowMap, RepositoryError> {
        Err(RepositoryError::Unimplemented(UNAVAILABLE))
    }
    async fn update(&self, _id: &Value, _data: RowMap) -> Result<Option<RowMap>, RepositoryError> {
        Err(RepositoryError::Unimplemented(UNAVAILABLE))
    }
    async fn delete(&self, _id: &Value) -> Result<bool, RepositoryError> {
        Err(RepositoryError::Unimplemented(UNAVAILABLE))
    }
}
