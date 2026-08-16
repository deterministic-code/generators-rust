use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::mappings::field_mapping_translator::FieldMappingTranslator;
use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::crud_repository::CrudRepository;
use crate::repositories::datasource_middleware::run_query_with_middlewares;
use crate::repositories::datasource_middleware::{DataSourceMiddleware, DatabaseBackend};
use crate::repositories::oracle::oracle_datasource::OracleDatasource;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::repositories::sql_builder::Dialect;
use crate::repositories::standard_crud_repository::StandardCrudRepository;

const DIALECT: Dialect = Dialect::Oracle;
const BACKEND: DatabaseBackend = DatabaseBackend::Oracle;
const UNAVAILABLE: &str = "oracle backend not available in Rust ecosystem";

pub struct OracleStandardRepository {
    datasource: Arc<OracleDatasource>,
    table_name: String,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
    field_mapping_translator: Arc<FieldMappingTranslator>,
    converters_by_type: BTreeMap<String, Arc<dyn TypeFieldConverter>>,
    column_datasource_types: BTreeMap<String, String>,
}

impl OracleStandardRepository {
    pub fn new(
        datasource: Arc<OracleDatasource>,
        table_name: impl Into<String>,
    ) -> Result<Self, RepositoryError> {
        let table_name = table_name.into();
        DIALECT.quote(&table_name)?;
        let passthrough_translator = Arc::new(FieldMappingTranslator::new(None)?);
        Ok(Self {
            datasource,
            table_name,
            middlewares: Vec::new(),
            field_mapping_translator: passthrough_translator,
            converters_by_type: BTreeMap::new(),
            column_datasource_types: BTreeMap::new(),
        })
    }

    pub fn with_middlewares(mut self, middlewares: Vec<Arc<dyn DataSourceMiddleware>>) -> Self {
        self.middlewares = middlewares;
        self
    }

    pub fn with_field_mapping_translator(
        mut self,
        translator: Arc<FieldMappingTranslator>,
    ) -> Self {
        self.field_mapping_translator = translator;
        self
    }

    pub fn with_converters(mut self, converters: Vec<Arc<dyn TypeFieldConverter>>) -> Self {
        self.converters_by_type = converters
            .into_iter()
            .map(|c| (c.datasource_type().to_string(), c))
            .collect();
        self
    }

    pub fn with_column_datasource_types(
        mut self,
        column_datasource_types: BTreeMap<String, String>,
    ) -> Self {
        self.column_datasource_types = column_datasource_types;
        self
    }

    pub fn table_name(&self) -> &str {
        &self.table_name
    }
}

#[async_trait]
impl Repository for OracleStandardRepository {
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
impl CrudRepository for OracleStandardRepository {
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

impl StandardCrudRepository for OracleStandardRepository {}
