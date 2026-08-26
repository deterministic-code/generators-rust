use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

use crate::error::RepositoryError;
use crate::id_type::IdType;
use crate::mappings::field_mapping_translator::FieldMappingTranslator;
use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::crud_repository::{
    bind_identity, fill_uuid_primary_key, identity_from_row, CrudRepository,
};
use crate::repositories::datasource::{Datasource, IntoDynDatasource};
use crate::repositories::datasource_middleware::run_query_with_middlewares;
use crate::repositories::datasource_middleware::{DataSourceMiddleware, DatabaseBackend};
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::repositories::sql_builder::{
    build_delete_by, build_delete_identity, build_insert, build_select_all, build_select_by_column,
    build_select_by_identity, build_select_in, build_update_by, build_update_identity, Dialect,
};

const DIALECT: Dialect = Dialect::Sqlite;
const BACKEND: DatabaseBackend = DatabaseBackend::Sqlite;

pub struct SqliteCrudRepository {
    datasource: Arc<dyn Datasource>,
    table_name: String,
    primary_keys: Vec<String>,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
    field_mapping_translator: Arc<FieldMappingTranslator>,
    converters_by_type: BTreeMap<String, Arc<dyn TypeFieldConverter>>,
    column_datasource_types: BTreeMap<String, String>,
    id_type: IdType,
    has_uuid_column: OnceLock<bool>,
}

impl SqliteCrudRepository {
    // why DynDatasource trait + IntoDynDatasource: lets callers pass either Arc<dyn Datasource> or Arc<SqliteDatasource> without manual `as Arc<dyn Datasource>` ceremony at every call site.
    pub fn new(
        datasource: impl IntoDynDatasource,
        table_name: impl Into<String>,
    ) -> Result<Self, RepositoryError> {
        let table_name = table_name.into();
        DIALECT.quote(&table_name)?;
        let passthrough = Arc::new(FieldMappingTranslator::new(None)?);
        Ok(Self {
            datasource: datasource.into_dyn_datasource(),
            table_name,
            primary_keys: vec!["id".to_string()],
            middlewares: Vec::new(),
            field_mapping_translator: passthrough,
            converters_by_type: BTreeMap::new(),
            column_datasource_types: BTreeMap::new(),
            id_type: IdType::default(),
            has_uuid_column: OnceLock::new(),
        })
    }

    pub fn with_primary_key(self, column: impl Into<String>) -> Result<Self, RepositoryError> {
        self.with_primary_keys([column.into()])
    }

    pub fn with_primary_keys<I, S>(mut self, columns: I) -> Result<Self, RepositoryError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let columns: Vec<String> = columns.into_iter().map(Into::into).collect();
        if columns.is_empty() {
            return Err(RepositoryError::Other(
                "primary key requires at least one column".into(),
            ));
        }
        for column in &columns {
            DIALECT.quote(column)?;
        }
        self.primary_keys = columns;
        Ok(self)
    }

    pub fn with_id_type(mut self, id_type: IdType) -> Self {
        self.id_type = id_type;
        self
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

    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    pub fn primary_key(&self) -> &str {
        self.primary_keys.first().map(String::as_str).unwrap_or("id")
    }

    fn physical_primary_keys(&self) -> Vec<String> {
        self.primary_keys
            .iter()
            .map(|c| self.field_mapping_translator.to_physical(c))
            .collect()
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

    // why client-side fill: SQLite has no UUID() builtin, so renamed uuid columns (e.g. notifications.guid) ship without DEFAULT and reject INSERTs that omit the field.
    async fn resolve_has_uuid_column(&self) -> Result<bool, RepositoryError> {
        if let Some(v) = self.has_uuid_column.get() {
            return Ok(*v);
        }
        let quoted_table = DIALECT.quote(&self.table_name)?;
        let sql = format!("PRAGMA table_info({quoted_table})");
        // why bypass middlewares: PRAGMA is an introspection probe, not user-visible traffic; counting it against the middleware contract would break observer parity with the TS repo (which also reads via the bare datasource).
        let rows = self.datasource.query(&sql, &[]).await?;
        let physical_uuid = self.field_mapping_translator.to_physical("uuid");
        let present = rows
            .iter()
            .any(|r| r.get("name").and_then(|v| v.as_str()) == Some(physical_uuid.as_str()));
        let _ = self.has_uuid_column.set(present);
        Ok(present)
    }
}

#[async_trait]
impl Repository for SqliteCrudRepository {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        self.run_query(sql, params).await
    }
}

#[async_trait]
impl CrudRepository for SqliteCrudRepository {
    fn primary_key_column(&self) -> &str {
        self.primary_key()
    }

    fn primary_key_columns(&self) -> Vec<String> {
        self.primary_keys.clone()
    }

    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let physical_pks = self.physical_primary_keys();
        let sql = build_select_by_identity(DIALECT, &self.table_name, &physical_pks)?;
        let params = bind_identity(id, &self.primary_keys)?;
        let rows = self.run_query(&sql, &params).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|r| self.apply_from_and_translate(r)))
    }

    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        let physical_pk = self.field_mapping_translator.to_physical(self.primary_key());
        let sql = build_select_all(DIALECT, &self.table_name, &physical_pk)?;
        let rows = self.run_query(&sql, &[]).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn find_by(&self, column: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        let physical_column = self.field_mapping_translator.to_physical(column);
        let physical_pk = self.field_mapping_translator.to_physical(self.primary_key());
        let sql =
            build_select_by_column(DIALECT, &self.table_name, &physical_column, &physical_pk)?;
        let bound_value = self.apply_to(column, value.clone());
        let rows = self.run_query(&sql, &[bound_value]).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn add(&self, data: RowMap) -> Result<RowMap, RepositoryError> {
        let mut data = data;
        fill_uuid_primary_key(&mut data, self.id_type, self.primary_key());
        if !data.contains_key("uuid") && self.resolve_has_uuid_column().await? {
            data.insert("uuid".to_string(), Value::from(Uuid::new_v4().to_string()));
        }
        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let values: Vec<Value> = prepared.values().cloned().collect();
        let physical_pk = self.field_mapping_translator.to_physical(self.primary_key());
        let sql = build_insert(DIALECT, &self.table_name, &columns, &physical_pk)?;
        let rows = self.run_query(&sql, &values).await?;
        let inserted = rows.into_iter().next().ok_or_else(|| {
            RepositoryError::Other("INSERT ... RETURNING * returned no row".into())
        })?;
        Ok(self.apply_from_and_translate(inserted))
    }

    async fn update(&self, id: &Value, data: RowMap) -> Result<Option<RowMap>, RepositoryError> {
        if data.is_empty() {
            return self.find(id).await;
        }
        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let mut values: Vec<Value> = prepared.values().cloned().collect();
        values.extend(bind_identity(id, &self.primary_keys)?);
        let physical_pks = self.physical_primary_keys();
        let sql = build_update_identity(DIALECT, &self.table_name, &columns, &physical_pks)?;
        self.run_query(&sql, &values).await?;
        self.find(id).await
    }

    async fn delete(&self, id: &Value) -> Result<bool, RepositoryError> {
        let physical_pks = self.physical_primary_keys();
        let sql = build_delete_identity(DIALECT, &self.table_name, &physical_pks)?;
        let params = bind_identity(id, &self.primary_keys)?;
        let rows = self.run_query(&sql, &params).await?;
        Ok(!rows.is_empty())
    }

    async fn update_with_expected(
        &self,
        id: &Value,
        data: RowMap,
        expected_updated: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        if let Some(expected) = expected_updated {
            let Some(current) = self.find(id).await? else {
                return Ok(None);
            };
            let actual = current.get("updated").and_then(|v| v.as_str());
            if actual != Some(expected) {
                return Err(RepositoryError::ConcurrencyConflict(format!(
                    "expected_updated \"{}\" does not match current \"{}\"",
                    expected,
                    actual.unwrap_or("")
                )));
            }
        }
        self.update(id, data).await
    }

    async fn delete_with_expected(
        &self,
        id: &Value,
        expected_updated: Option<&str>,
    ) -> Result<bool, RepositoryError> {
        if let Some(expected) = expected_updated {
            let Some(current) = self.find(id).await? else {
                return Ok(false);
            };
            let actual = current.get("updated").and_then(|v| v.as_str());
            if actual != Some(expected) {
                return Err(RepositoryError::ConcurrencyConflict(format!(
                    "expected_updated \"{}\" does not match current \"{}\"",
                    expected,
                    actual.unwrap_or("")
                )));
            }
        }
        self.delete(id).await
    }

    async fn find_in(
        &self,
        column: &str,
        values: &[Value],
    ) -> Result<Vec<RowMap>, RepositoryError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let physical_column = self.field_mapping_translator.to_physical(column);
        let physical_pk = self.field_mapping_translator.to_physical(self.primary_key());
        let sql = build_select_in(
            DIALECT,
            &self.table_name,
            &physical_column,
            values.len(),
            &physical_pk,
        )?;
        let bound: Vec<Value> = values
            .iter()
            .map(|v| self.apply_to(column, v.clone()))
            .collect();
        let rows = self.run_query(&sql, &bound).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn update_by(
        &self,
        column: &str,
        value: &Value,
        data: RowMap,
    ) -> Result<Vec<RowMap>, RepositoryError> {
        if data.is_empty() {
            return self.find_by(column, value).await;
        }
        let matched = self.find_by(column, value).await?;
        if matched.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Value> = matched
            .iter()
            .map(|r| identity_from_row(r, &self.primary_keys))
            .collect();
        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let mut params: Vec<Value> = prepared.values().cloned().collect();
        let bound_match = self.apply_to(column, value.clone());
        params.push(bound_match);
        let physical_column = self.field_mapping_translator.to_physical(column);
        let sql = build_update_by(DIALECT, &self.table_name, &columns, &physical_column)?;
        self.run_query(&sql, &params).await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(row) = self.find(&id).await? {
                out.push(row);
            }
        }
        Ok(out)
    }

    async fn delete_by(&self, column: &str, value: &Value) -> Result<u64, RepositoryError> {
        let matched = self.find_by(column, value).await?;
        if matched.is_empty() {
            return Ok(0);
        }
        let physical_column = self.field_mapping_translator.to_physical(column);
        let sql = build_delete_by(DIALECT, &self.table_name, &physical_column)?;
        let bound = self.apply_to(column, value.clone());
        self.run_query(&sql, &[bound]).await?;
        Ok(matched.len() as u64)
    }
}
