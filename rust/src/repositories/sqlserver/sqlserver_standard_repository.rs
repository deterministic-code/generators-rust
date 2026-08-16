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
use crate::repositories::sql_builder::{build_insert, build_update, Dialect};
use crate::repositories::sqlserver::sqlserver_datasource::SqlserverDatasource;
use crate::repositories::standard_crud_repository::StandardCrudRepository;
use crate::sql_identifier::validate_identifier;

const DIALECT: Dialect = Dialect::Sqlserver;
const BACKEND: DatabaseBackend = DatabaseBackend::Sqlserver;

pub struct SqlserverStandardRepository {
    datasource: Arc<SqlserverDatasource>,
    table_name: String,
    entity_name: String,
    entity_name_plural: String,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
    field_mapping_translator: Arc<FieldMappingTranslator>,
    converters_by_type: BTreeMap<String, Arc<dyn TypeFieldConverter>>,
    column_datasource_types: BTreeMap<String, String>,
    use_stored_procedures: bool,
    use_optimistic_concurrency: bool,
}

impl SqlserverStandardRepository {
    pub fn new(
        datasource: Arc<SqlserverDatasource>,
        table_name: impl Into<String>,
    ) -> Result<Self, RepositoryError> {
        let table_name = table_name.into();
        DIALECT.quote(&table_name)?;
        let entity_name = table_name.clone();
        let entity_name_plural = format!("{}s", table_name);
        let passthrough_translator = Arc::new(FieldMappingTranslator::new(None)?);
        Ok(Self {
            datasource,
            table_name,
            entity_name,
            entity_name_plural,
            middlewares: Vec::new(),
            field_mapping_translator: passthrough_translator,
            converters_by_type: BTreeMap::new(),
            column_datasource_types: BTreeMap::new(),
            use_stored_procedures: false,
            use_optimistic_concurrency: false,
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

    pub fn with_stored_procedures(mut self, enabled: bool) -> Self {
        self.use_stored_procedures = enabled;
        self
    }

    pub fn with_optimistic_concurrency(mut self, enabled: bool) -> Self {
        self.use_optimistic_concurrency = enabled;
        self
    }

    pub fn with_entity_name(mut self, entity_name: impl Into<String>) -> Self {
        self.entity_name = entity_name.into();
        self
    }

    pub fn with_entity_name_plural(mut self, entity_name_plural: impl Into<String>) -> Self {
        self.entity_name_plural = entity_name_plural.into();
        self
    }

    pub fn validate(&self) -> Result<(), RepositoryError> {
        if self.use_optimistic_concurrency && !self.use_stored_procedures {
            return Err(RepositoryError::InvalidConfig(
                "useOptimisticConcurrency requires useStoredProcedures: enable both or neither"
                    .to_string(),
            ));
        }
        Ok(())
    }

    pub fn build_add_sql(&self, columns: &[&str]) -> Result<String, RepositoryError> {
        build_insert(DIALECT, &self.table_name, columns, "id")
    }
    pub fn build_update_sql(&self, columns: &[&str]) -> Result<String, RepositoryError> {
        build_update(DIALECT, &self.table_name, columns, "id")
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

    fn build_exec_sql(proc_name: &str, params_len: usize) -> String {
        if params_len == 0 {
            return format!("EXEC {}", proc_name);
        }
        let placeholders: Vec<String> = (1..=params_len).map(|i| format!("@p{}", i)).collect();
        format!("EXEC {} {}", proc_name, placeholders.join(", "))
    }

    async fn invoke_returning_id(
        &self,
        proc_name: &str,
        params: &[Value],
    ) -> Result<i64, RepositoryError> {
        let sql = Self::build_exec_sql(proc_name, params.len());
        let rows = self.run_query(&sql, params).await?;
        let head = rows.into_iter().next().ok_or_else(|| {
            RepositoryError::Other(format!("{} returned no row for id", proc_name))
        })?;
        head.get("id").and_then(|v| v.as_i64()).ok_or_else(|| {
            RepositoryError::Other(format!("{} did not return id in recordset", proc_name))
        })
    }

    async fn invoke_returning_affected(
        &self,
        proc_name: &str,
        params: &[Value],
    ) -> Result<i64, RepositoryError> {
        let sql = Self::build_exec_sql(proc_name, params.len());
        let rows = self.run_query(&sql, params).await?;
        let head = rows.into_iter().next().ok_or_else(|| {
            RepositoryError::Other(format!("{} returned no row for affected", proc_name))
        })?;
        head.get("affected")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                RepositoryError::Other(format!(
                    "{} did not return affected in recordset",
                    proc_name
                ))
            })
    }

    async fn invoke_returning_rows(
        &self,
        proc_name: &str,
        params: &[Value],
    ) -> Result<Vec<RowMap>, RepositoryError> {
        let sql = Self::build_exec_sql(proc_name, params.len());
        self.run_query(&sql, params).await
    }

    pub async fn update_with_expected(
        &self,
        id: i64,
        data: RowMap,
        expected_updated: &str,
    ) -> Result<Option<RowMap>, RepositoryError> {
        self.validate()?;
        if !self.use_stored_procedures || !self.use_optimistic_concurrency {
            return Err(RepositoryError::InvalidConfig(
                "update_with_expected requires useStoredProcedures and useOptimisticConcurrency"
                    .to_string(),
            ));
        }
        let now = now_iso();
        let name = data.get("name").cloned().unwrap_or(Value::Null);
        let email = data.get("email").cloned().unwrap_or(Value::Null);
        let proc = format!("update_{}_optimistic_concurrency", self.entity_name);
        let affected = self
            .invoke_returning_affected(
                &proc,
                &[
                    Value::from(id),
                    Value::from(expected_updated.to_string()),
                    name,
                    email,
                    Value::from(now),
                ],
            )
            .await?;
        if affected != 1 {
            return Err(RepositoryError::ConcurrencyConflict(format!(
                "{} affected {} rows; expected 1",
                proc, affected
            )));
        }
        self.find(&Value::from(id)).await
    }

    pub async fn delete_with_expected(
        &self,
        id: i64,
        expected_updated: &str,
    ) -> Result<bool, RepositoryError> {
        self.validate()?;
        if !self.use_stored_procedures || !self.use_optimistic_concurrency {
            return Err(RepositoryError::InvalidConfig(
                "delete_with_expected requires useStoredProcedures and useOptimisticConcurrency"
                    .to_string(),
            ));
        }
        let proc = format!("delete_{}_optimistic_concurrency", self.entity_name);
        let affected = self
            .invoke_returning_affected(
                &proc,
                &[Value::from(id), Value::from(expected_updated.to_string())],
            )
            .await?;
        if affected != 1 {
            return Err(RepositoryError::ConcurrencyConflict(format!(
                "{} affected {} rows; expected 1",
                proc, affected
            )));
        }
        Ok(true)
    }
}

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[async_trait]
impl Repository for SqlserverStandardRepository {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        self.run_query(sql, params).await
    }
}

#[async_trait]
impl CrudRepository for SqlserverStandardRepository {
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        self.validate()?;
        if self.use_stored_procedures {
            let proc = format!("find_{}", self.entity_name);
            let rows = self
                .invoke_returning_rows(&proc, std::slice::from_ref(id))
                .await?;
            return Ok(rows
                .into_iter()
                .next()
                .map(|r| self.apply_from_and_translate(r)));
        }
        let sql = format!(
            "SELECT * FROM {} WHERE [id] = @p1",
            DIALECT.quote(&self.table_name)?
        );
        let rows = self.run_query(&sql, std::slice::from_ref(id)).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|r| self.apply_from_and_translate(r)))
    }

    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        self.validate()?;
        if self.use_stored_procedures {
            let proc = format!("find_{}", self.entity_name_plural);
            let rows = self.invoke_returning_rows(&proc, &[]).await?;
            return Ok(rows
                .into_iter()
                .map(|r| self.apply_from_and_translate(r))
                .collect());
        }
        let sql = format!(
            "SELECT * FROM {} ORDER BY [id] ASC",
            DIALECT.quote(&self.table_name)?
        );
        let rows = self.run_query(&sql, &[]).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn find_by(&self, column: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        self.validate()?;
        if self.use_stored_procedures {
            validate_identifier(column)?;
            let proc = format!("find_{}_by_{}", self.entity_name, column);
            let bound_value = self.apply_to(column, value.clone());
            let rows = self.invoke_returning_rows(&proc, &[bound_value]).await?;
            return Ok(rows
                .into_iter()
                .map(|r| self.apply_from_and_translate(r))
                .collect());
        }
        let physical_column = self.field_mapping_translator.to_physical(column);
        let bound_value = self.apply_to(column, value.clone());
        let sql = format!(
            "SELECT * FROM {} WHERE {} = @p1 ORDER BY [id] ASC",
            DIALECT.quote(&self.table_name)?,
            DIALECT.quote(&physical_column)?
        );
        let rows = self.run_query(&sql, &[bound_value]).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn add(&self, data: RowMap) -> Result<RowMap, RepositoryError> {
        self.validate()?;
        let now = now_iso();
        let mut data = data;
        data.insert("uuid".to_string(), Value::from(Uuid::new_v4().to_string()));
        data.insert("created".to_string(), Value::from(now.clone()));
        data.insert("updated".to_string(), Value::from(now));

        if self.use_stored_procedures {
            let proc = format!("create_{}", self.entity_name);
            let uuid_v = data.get("uuid").cloned().unwrap_or(Value::Null);
            let name_v = data.get("name").cloned().unwrap_or(Value::Null);
            let email_v = data.get("email").cloned().unwrap_or(Value::Null);
            let created_v = data.get("created").cloned().unwrap_or(Value::Null);
            let updated_v = data.get("updated").cloned().unwrap_or(Value::Null);
            let new_id = self
                .invoke_returning_id(&proc, &[uuid_v, name_v, email_v, created_v, updated_v])
                .await?;
            return self
                .find(&Value::from(new_id))
                .await?
                .ok_or(RepositoryError::InsertedRowMissing(new_id));
        }

        Err(RepositoryError::Unimplemented(
            "SqlserverStandardRepository.add inline path pending tiberius wiring",
        ))
    }

    async fn update(&self, id: &Value, data: RowMap) -> Result<Option<RowMap>, RepositoryError> {
        self.validate()?;
        let mut data = data;
        data.insert("updated".to_string(), Value::from(now_iso()));

        if self.use_stored_procedures {
            let proc = format!("update_{}", self.entity_name);
            let name_v = data.get("name").cloned().unwrap_or(Value::Null);
            let email_v = data.get("email").cloned().unwrap_or(Value::Null);
            let updated_v = data.get("updated").cloned().unwrap_or(Value::Null);
            let affected = self
                .invoke_returning_affected(&proc, &[id.clone(), name_v, email_v, updated_v])
                .await?;
            if affected == 0 {
                return Ok(None);
            }
            return self.find(id).await;
        }

        Err(RepositoryError::Unimplemented(
            "SqlserverStandardRepository.update inline path pending tiberius wiring",
        ))
    }

    async fn delete(&self, id: &Value) -> Result<bool, RepositoryError> {
        self.validate()?;
        if self.use_stored_procedures {
            let proc = format!("delete_{}", self.entity_name);
            let affected = self
                .invoke_returning_affected(&proc, std::slice::from_ref(id))
                .await?;
            return Ok(affected > 0);
        }
        Err(RepositoryError::Unimplemented(
            "SqlserverStandardRepository.delete inline path pending tiberius wiring",
        ))
    }
}

impl StandardCrudRepository for SqlserverStandardRepository {}
