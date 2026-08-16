use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::id_type::IdType;
use crate::mappings::field_mapping_translator::FieldMappingTranslator;
use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::crud_repository::{fill_uuid_primary_key, CrudRepository};
use crate::repositories::datasource::{Datasource, IntoDynDatasource};
use crate::repositories::datasource_middleware::run_query_with_middlewares;
use crate::repositories::datasource_middleware::{DataSourceMiddleware, DatabaseBackend};
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::repositories::sql_builder::{
    build_delete, build_delete_by, build_insert, build_select_all, build_select_by_column,
    build_select_by_id, build_select_in, build_update, build_update_by, Dialect,
};

const DIALECT: Dialect = Dialect::Mysql;
const BACKEND: DatabaseBackend = DatabaseBackend::Mysql;

/// mysql's `LAST_INSERT_ID()` returns 0 for non-AUTO_INCREMENT primary keys, so only an implicit
/// integer `id` can be read back that way — a uuid or custom PK round-trips the key we already hold.
fn uses_autoincrement_readback(primary_key: &str, id_type: IdType) -> bool {
    primary_key == "id" && !id_type.is_uuid()
}

pub struct MysqlCrudRepository {
    datasource: Arc<dyn Datasource>,
    table_name: String,
    primary_key: String,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
    field_mapping_translator: Arc<FieldMappingTranslator>,
    converters_by_type: BTreeMap<String, Arc<dyn TypeFieldConverter>>,
    column_datasource_types: BTreeMap<String, String>,
    id_type: IdType,
}

impl MysqlCrudRepository {
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
            primary_key: "id".to_string(),
            middlewares: Vec::new(),
            field_mapping_translator: passthrough,
            converters_by_type: BTreeMap::new(),
            column_datasource_types: BTreeMap::new(),
            id_type: IdType::default(),
        })
    }

    pub fn with_primary_key(mut self, column: impl Into<String>) -> Result<Self, RepositoryError> {
        let column = column.into();
        DIALECT.quote(&column)?;
        self.primary_key = column;
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

    // why converters: MySQL DATETIME rejects the ISO strings sqlx binds as TEXT (error 1292) — MysqlDateTimeConverter normalizes them at bind time.
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
        &self.primary_key
    }

    pub fn build_find_sql(&self) -> Result<String, RepositoryError> {
        build_select_by_id(DIALECT, &self.table_name, &self.primary_key)
    }
    pub fn build_find_all_sql(&self) -> Result<String, RepositoryError> {
        build_select_all(DIALECT, &self.table_name, &self.primary_key)
    }
    pub fn build_find_by_sql(&self, column: &str) -> Result<String, RepositoryError> {
        build_select_by_column(DIALECT, &self.table_name, column, &self.primary_key)
    }
    pub fn build_add_sql(&self, columns: &[&str]) -> Result<String, RepositoryError> {
        build_insert(DIALECT, &self.table_name, columns, &self.primary_key)
    }
    pub fn build_update_sql(&self, columns: &[&str]) -> Result<String, RepositoryError> {
        build_update(DIALECT, &self.table_name, columns, &self.primary_key)
    }
    pub fn build_delete_sql(&self) -> Result<String, RepositoryError> {
        build_delete(DIALECT, &self.table_name, &self.primary_key)
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
}

#[async_trait]
impl Repository for MysqlCrudRepository {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        self.run_query(sql, params).await
    }
}

#[async_trait]
impl CrudRepository for MysqlCrudRepository {
    fn primary_key_column(&self) -> &str {
        &self.primary_key
    }

    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let physical_pk = self.field_mapping_translator.to_physical(&self.primary_key);
        let sql = build_select_by_id(DIALECT, &self.table_name, &physical_pk)?;
        let rows = self.run_query(&sql, std::slice::from_ref(id)).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|r| self.apply_from_and_translate(r)))
    }

    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        let physical_pk = self.field_mapping_translator.to_physical(&self.primary_key);
        let sql = build_select_all(DIALECT, &self.table_name, &physical_pk)?;
        let rows = self.run_query(&sql, &[]).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn find_by(&self, column: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        let physical_column = self.field_mapping_translator.to_physical(column);
        let physical_pk = self.field_mapping_translator.to_physical(&self.primary_key);
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
        fill_uuid_primary_key(&mut data, self.id_type, &self.primary_key);
        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let values: Vec<Value> = prepared.values().cloned().collect();
        let physical_pk = self.field_mapping_translator.to_physical(&self.primary_key);
        let sql = build_insert(DIALECT, &self.table_name, &columns, &physical_pk)?;
        // why split: mysql's LAST_INSERT_ID() returns 0 for non-AUTO_INCREMENT PKs (custom or uuid keys), so we round-trip the client/generated key for those tables.
        let returned_pk = if uses_autoincrement_readback(&self.primary_key, self.id_type) {
            let new_id = self
                .datasource
                .execute_insert_returning_id(&sql, &values)
                .await?;
            Value::from(new_id)
        } else {
            self.run_query(&sql, &values).await?;
            prepared.get(&physical_pk).cloned().ok_or_else(|| {
                RepositoryError::Other(format!(
                    "add: row missing primary-key column {:?}",
                    self.primary_key
                ))
            })?
        };
        self.find(&returned_pk)
            .await?
            .ok_or_else(|| match &returned_pk {
                Value::Number(n) => n
                    .as_i64()
                    .map(RepositoryError::InsertedRowMissing)
                    .unwrap_or_else(|| {
                        RepositoryError::Other(format!("inserted row missing (pk={returned_pk})"))
                    }),
                _ => RepositoryError::Other(format!("inserted row missing (pk={returned_pk})")),
            })
    }

    async fn update(&self, id: &Value, data: RowMap) -> Result<Option<RowMap>, RepositoryError> {
        if data.is_empty() {
            return self.find(id).await;
        }
        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let mut values: Vec<Value> = prepared.values().cloned().collect();
        values.push(id.clone());
        let physical_pk = self.field_mapping_translator.to_physical(&self.primary_key);
        let sql = build_update(DIALECT, &self.table_name, &columns, &physical_pk)?;
        self.run_query(&sql, &values).await?;
        self.find(id).await
    }

    async fn delete(&self, id: &Value) -> Result<bool, RepositoryError> {
        let existed = self.find(id).await?.is_some();
        let physical_pk = self.field_mapping_translator.to_physical(&self.primary_key);
        let sql = build_delete(DIALECT, &self.table_name, &physical_pk)?;
        self.run_query(&sql, std::slice::from_ref(id)).await?;
        Ok(existed)
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
        let physical_pk = self.field_mapping_translator.to_physical(&self.primary_key);
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
            .filter_map(|r| r.get(&self.primary_key).cloned())
            .collect();
        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let mut params: Vec<Value> = prepared.values().cloned().collect();
        let bound_match = self.apply_to(column, value.clone());
        params.push(bound_match);
        let physical_column = self.field_mapping_translator.to_physical(column);
        let sql = build_update_by(DIALECT, &self.table_name, &columns, &physical_column)?;
        self.run_query(&sql, &params).await?;
        self.find_in(&self.primary_key, &ids).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::mysql_date_time_converter::MysqlDateTimeConverter;
    use crate::repositories::mysql::mysql_datasource::{MysqlDatasource, MysqlDatasourceOptions};
    use serde_json::json;

    fn unopened_datasource() -> Arc<dyn Datasource> {
        Arc::new(MysqlDatasource::new(MysqlDatasourceOptions {
            url: "mysql://test:test@127.0.0.1/none".to_string(),
            max_connections: 1,
        }))
    }

    #[test]
    fn autoincrement_readback_only_for_implicit_integer_id() {
        assert!(uses_autoincrement_readback("id", IdType::Integer));
        assert!(!uses_autoincrement_readback("id", IdType::Uuid));
        assert!(!uses_autoincrement_readback("key", IdType::Integer));
    }

    fn legacy_contact_repo() -> MysqlCrudRepository {
        let mut entity_map = crate::mappings::EntityFieldMap::new();
        entity_map.insert("key".to_string(), "CntID".to_string());
        entity_map.insert("first_name".to_string(), "FirstNm".to_string());
        entity_map.insert("last_name".to_string(), "LastNm".to_string());
        entity_map.insert("email".to_string(), "EmailAddr".to_string());
        entity_map.insert("imported_at".to_string(), "ImpDate".to_string());
        let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).unwrap());
        let mut col_types = BTreeMap::new();
        col_types.insert("imported_at".to_string(), "datetime".to_string());
        MysqlCrudRepository::new(unopened_datasource(), "OldContactsTbl")
            .unwrap()
            .with_primary_key("key")
            .unwrap()
            .with_field_mapping_translator(translator)
            .with_column_datasource_types(col_types)
            .with_converters(vec![
                Arc::new(MysqlDateTimeConverter) as Arc<dyn TypeFieldConverter>
            ])
    }

    #[test]
    fn prepare_write_row_normalizes_iso_datetime_for_mapped_column() {
        let repo = legacy_contact_repo();
        let mut row = RowMap::new();
        row.insert("key".to_string(), json!("sample-0"));
        row.insert("first_name".to_string(), json!("sample-0"));
        row.insert("last_name".to_string(), json!("sample-0"));
        row.insert("email".to_string(), json!("sample-0"));
        row.insert("imported_at".to_string(), json!("2024-01-01T00:00:00.000Z"));
        let prepared = repo.prepare_write_row(row);

        // logical → physical column rename happened
        assert_eq!(
            prepared.get("CntID").and_then(|v| v.as_str()),
            Some("sample-0")
        );
        assert_eq!(
            prepared.get("FirstNm").and_then(|v| v.as_str()),
            Some("sample-0")
        );
        assert_eq!(
            prepared.get("EmailAddr").and_then(|v| v.as_str()),
            Some("sample-0")
        );
        // datetime converter normalized the bind value (T→space, drop Z)
        assert_eq!(
            prepared.get("ImpDate").and_then(|v| v.as_str()),
            Some("2024-01-01 00:00:00.000"),
        );
    }

    #[test]
    fn prepare_write_row_passes_through_when_no_converters_registered() {
        let mut entity_map = crate::mappings::EntityFieldMap::new();
        entity_map.insert("imported_at".to_string(), "ImpDate".to_string());
        let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).unwrap());
        let repo = MysqlCrudRepository::new(unopened_datasource(), "OldContactsTbl")
            .unwrap()
            .with_field_mapping_translator(translator);
        let mut row = RowMap::new();
        row.insert("imported_at".to_string(), json!("2024-01-01T00:00:00.000Z"));
        let prepared = repo.prepare_write_row(row);
        // no converter → ISO string passes through verbatim (legacy behavior)
        assert_eq!(
            prepared.get("ImpDate").and_then(|v| v.as_str()),
            Some("2024-01-01T00:00:00.000Z"),
        );
    }

    #[test]
    fn prepare_write_row_leaves_null_datetime_alone() {
        let repo = legacy_contact_repo();
        let mut row = RowMap::new();
        row.insert("key".to_string(), json!("sample-1"));
        row.insert("imported_at".to_string(), Value::Null);
        let prepared = repo.prepare_write_row(row);
        assert!(prepared
            .get("ImpDate")
            .map(|v| v.is_null())
            .unwrap_or(false));
    }
}
