use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::RepositoryError;
use crate::mappings::field_mapping_translator::FieldMappingTranslator;
use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::crud_repository::CrudRepository;
use crate::repositories::datasource_middleware::run_query_with_middlewares;
use crate::repositories::datasource_middleware::{DataSourceMiddleware, DatabaseBackend};
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::repositories::sql_builder::{
    build_delete, build_insert, build_select_all, build_select_by_column, build_select_by_id,
    build_update, Dialect,
};
use crate::repositories::sqlite::sqlite_datasource::SqliteDatasource;
use crate::repositories::standard_crud_repository::StandardCrudRepository;

const DIALECT: Dialect = Dialect::Sqlite;
const BACKEND: DatabaseBackend = DatabaseBackend::Sqlite;

pub struct SqliteStandardRepository {
    datasource: Arc<SqliteDatasource>,
    table_name: String,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
    field_mapping_translator: Arc<FieldMappingTranslator>,
    converters_by_type: BTreeMap<String, Arc<dyn TypeFieldConverter>>,
    column_datasource_types: BTreeMap<String, String>,
    use_stored_procedures: bool,
    use_optimistic_concurrency: bool,
}

impl SqliteStandardRepository {
    pub fn new(
        datasource: Arc<SqliteDatasource>,
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
            use_stored_procedures: false,
            use_optimistic_concurrency: false,
        })
    }

    pub fn with_stored_procedures(mut self, enabled: bool) -> Self {
        self.use_stored_procedures = enabled;
        self
    }

    pub fn with_optimistic_concurrency(mut self, enabled: bool) -> Self {
        self.use_optimistic_concurrency = enabled;
        self
    }

    pub fn validate(&self) -> Result<(), RepositoryError> {
        if self.use_stored_procedures {
            return Err(RepositoryError::InvalidConfig(
                "sqlite does not support stored procedures".to_string(),
            ));
        }
        if self.use_optimistic_concurrency && !self.use_stored_procedures {
            return Err(RepositoryError::InvalidConfig(
                "useOptimisticConcurrency requires useStoredProcedures: enable both or neither"
                    .to_string(),
            ));
        }
        Ok(())
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

    async fn run_query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        run_query_with_middlewares(
            &self.middlewares,
            BACKEND,
            self.datasource.as_ref(),
            sql,
            params,
        )
        .await
    }

    fn apply_to(&self, logical_column: &str, value: Value) -> Value {
        if self.column_datasource_types.is_empty() {
            return value;
        }
        let Some(ds_type) = self.column_datasource_types.get(logical_column) else {
            return value;
        };
        let Some(conv) = self.converters_by_type.get(ds_type) else {
            return value;
        };
        conv.to(&value)
    }

    fn apply_from_and_translate(&self, row: RowMap) -> RowMap {
        let logical_row = self.field_mapping_translator.to_logical_row(row);
        if self.column_datasource_types.is_empty() || self.converters_by_type.is_empty() {
            return logical_row;
        }
        let mut out = RowMap::new();
        for (logical_key, val) in logical_row {
            let converted = match self
                .column_datasource_types
                .get(&logical_key)
                .and_then(|t| self.converters_by_type.get(t))
            {
                Some(conv) => conv.from(&val),
                None => val,
            };
            out.insert(logical_key, converted);
        }
        out
    }

    fn prepare_write_row(&self, row: RowMap) -> RowMap {
        let converted: RowMap = row
            .into_iter()
            .map(|(k, v)| {
                let new_value = self.apply_to(&k, v);
                (k, new_value)
            })
            .collect();
        self.field_mapping_translator.to_physical_row(converted)
    }
}

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[async_trait]
impl Repository for SqliteStandardRepository {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        self.run_query(sql, params).await
    }
}

#[async_trait]
impl CrudRepository for SqliteStandardRepository {
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let sql = build_select_by_id(DIALECT, &self.table_name, "id")?;
        let rows = self.run_query(&sql, std::slice::from_ref(id)).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|r| self.apply_from_and_translate(r)))
    }

    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        let sql = build_select_all(DIALECT, &self.table_name, "id")?;
        let rows = self.run_query(&sql, &[]).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn find_by(&self, column: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        let physical_column = self.field_mapping_translator.to_physical(column);
        let bound_value = self.apply_to(column, value.clone());
        let sql = build_select_by_column(DIALECT, &self.table_name, &physical_column, "id")?;
        let rows = self.run_query(&sql, &[bound_value]).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn add(&self, data: RowMap) -> Result<RowMap, RepositoryError> {
        let now = now_iso();
        let mut data = data;
        data.insert("uuid".to_string(), Value::from(Uuid::new_v4().to_string()));
        data.insert("created".to_string(), Value::from(now.clone()));
        data.insert("updated".to_string(), Value::from(now));

        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let values: Vec<Value> = prepared.values().cloned().collect();
        let sql = build_insert(DIALECT, &self.table_name, &columns, "id")?;
        let rows = self.run_query(&sql, &values).await?;
        let inserted = rows.into_iter().next().ok_or_else(|| {
            RepositoryError::Other("INSERT ... RETURNING * returned no row".into())
        })?;
        Ok(self.apply_from_and_translate(inserted))
    }

    async fn update(&self, id: &Value, data: RowMap) -> Result<Option<RowMap>, RepositoryError> {
        let mut data = data;
        data.insert("updated".to_string(), Value::from(now_iso()));

        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let mut values: Vec<Value> = prepared.values().cloned().collect();
        values.push(id.clone());
        let sql = build_update(DIALECT, &self.table_name, &columns, "id")?;
        self.run_query(&sql, &values).await?;
        self.find(id).await
    }

    async fn delete(&self, id: &Value) -> Result<bool, RepositoryError> {
        let sql = build_delete(DIALECT, &self.table_name, "id")?;
        let rows = self.run_query(&sql, std::slice::from_ref(id)).await?;
        Ok(!rows.is_empty())
    }
}

impl StandardCrudRepository for SqliteStandardRepository {}
