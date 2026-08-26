use async_trait::async_trait;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::id_type::IdType;
use crate::mappings::field_mapping_translator::FieldMappingTranslator;
use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::crud_repository::{
    bind_identity, fill_uuid_primary_key, CrudRepository,
};
use crate::repositories::datasource::{Datasource, IntoDynDatasource};
use crate::repositories::datasource_middleware::run_query_with_middlewares;
use crate::repositories::datasource_middleware::{DataSourceMiddleware, DatabaseBackend};
use crate::repositories::postgres::pg_converting_repo::PgConvertingRepo;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::repositories::sql_builder::{
    build_delete_by, build_delete_identity, build_insert, build_select_all,
    build_select_by_column, build_select_by_identity, build_select_in, build_update_by,
    build_update_identity, Dialect,
};

const DIALECT: Dialect = Dialect::Postgres;
const BACKEND: DatabaseBackend = DatabaseBackend::Postgres;

pub struct PostgresCrudRepository {
    datasource: Arc<dyn Datasource>,
    table_name: String,
    primary_keys: Vec<String>,
    middlewares: Vec<Arc<dyn DataSourceMiddleware>>,
    field_mapping_translator: Arc<FieldMappingTranslator>,
    converters_by_type: BTreeMap<String, Arc<dyn TypeFieldConverter>>,
    column_datasource_types: BTreeMap<String, String>,
    id_type: IdType,
}

impl PostgresCrudRepository {
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

    // why converters: postgres rejects text bindings for TIMESTAMPTZ/UUID (unlike sqlite/mysql implicit coercion) — per-column converters add ::timestamptz/::uuid bind casts.
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

    pub fn primary_key(&self) -> &str {
        self.primary_keys.first().map(String::as_str).unwrap_or("id")
    }

    fn physical_primary_keys(&self) -> Vec<String> {
        self.primary_keys
            .iter()
            .map(|c| self.field_mapping_translator.to_physical(c))
            .collect()
    }

    pub fn build_find_sql(&self) -> Result<String, RepositoryError> {
        build_select_by_identity(DIALECT, &self.table_name, &self.primary_keys)
    }
    pub fn build_find_all_sql(&self) -> Result<String, RepositoryError> {
        build_select_all(DIALECT, &self.table_name, self.primary_key())
    }
    pub fn build_find_by_sql(&self, column: &str) -> Result<String, RepositoryError> {
        build_select_by_column(DIALECT, &self.table_name, column, self.primary_key())
    }
    pub fn build_add_sql(&self, columns: &[&str]) -> Result<String, RepositoryError> {
        build_insert(DIALECT, &self.table_name, columns, self.primary_key())
    }
    pub fn build_update_sql(&self, columns: &[&str]) -> Result<String, RepositoryError> {
        build_update_identity(DIALECT, &self.table_name, columns, &self.primary_keys)
    }
    pub fn build_delete_sql(&self) -> Result<String, RepositoryError> {
        build_delete_identity(DIALECT, &self.table_name, &self.primary_keys)
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

impl PgConvertingRepo for PostgresCrudRepository {
    fn column_datasource_types(&self) -> &BTreeMap<String, String> {
        &self.column_datasource_types
    }
    fn converters_by_type(&self) -> &BTreeMap<String, Arc<dyn TypeFieldConverter>> {
        &self.converters_by_type
    }
    fn field_mapping_translator(&self) -> &Arc<FieldMappingTranslator> {
        &self.field_mapping_translator
    }
}

#[async_trait]
impl Repository for PostgresCrudRepository {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        self.run_query(sql, params).await
    }
}

#[async_trait]
impl CrudRepository for PostgresCrudRepository {
    fn primary_key_column(&self) -> &str {
        self.primary_key()
    }

    fn primary_key_columns(&self) -> Vec<String> {
        self.primary_keys.clone()
    }

    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let physical_pks = self.physical_primary_keys();
        let sql = build_select_by_identity(DIALECT, &self.table_name, &physical_pks)?;
        let cast_columns: Vec<&str> = physical_pks.iter().map(String::as_str).collect();
        let sql = PgConvertingRepo::apply_converter_bind_casts(self, sql, &cast_columns);
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
        let sql =
            PgConvertingRepo::apply_converter_bind_casts(self, sql, &[physical_column.as_str()]);
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
        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let values: Vec<Value> = prepared.values().cloned().collect();
        let physical_pk = self.field_mapping_translator.to_physical(self.primary_key());
        let sql = build_insert(DIALECT, &self.table_name, &columns, &physical_pk)?;
        let sql = PgConvertingRepo::apply_converter_bind_casts(self, sql, &columns);
        let rows = self.run_query(&sql, &values).await?;
        rows.into_iter()
            .next()
            .map(|r| self.apply_from_and_translate(r))
            .ok_or(RepositoryError::InsertedRowMissing(0))
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
        let mut cast_columns = columns.clone();
        cast_columns.extend(physical_pks.iter().map(String::as_str));
        let sql = PgConvertingRepo::apply_converter_bind_casts(self, sql, &cast_columns);
        let rows = self.run_query(&sql, &values).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|r| self.apply_from_and_translate(r)))
    }

    async fn delete(&self, id: &Value) -> Result<bool, RepositoryError> {
        let physical_pks = self.physical_primary_keys();
        let sql = build_delete_identity(DIALECT, &self.table_name, &physical_pks)?;
        let cast_columns: Vec<&str> = physical_pks.iter().map(String::as_str).collect();
        let sql = PgConvertingRepo::apply_converter_bind_casts(self, sql, &cast_columns);
        let params = bind_identity(id, &self.primary_keys)?;
        let rows = self.run_query(&sql, &params).await?;
        Ok(!rows.is_empty())
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
        let cast_columns = vec![physical_column.as_str(); values.len()];
        let sql = PgConvertingRepo::apply_converter_bind_casts(self, sql, &cast_columns);
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
        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let mut params: Vec<Value> = prepared.values().cloned().collect();
        let bound_match = self.apply_to(column, value.clone());
        params.push(bound_match);
        let physical_column = self.field_mapping_translator.to_physical(column);
        let sql = build_update_by(DIALECT, &self.table_name, &columns, &physical_column)?;
        let mut cast_columns = columns.clone();
        cast_columns.push(physical_column.as_str());
        let sql = PgConvertingRepo::apply_converter_bind_casts(self, sql, &cast_columns);
        let rows = self.run_query(&sql, &params).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.apply_from_and_translate(r))
            .collect())
    }

    async fn delete_by(&self, column: &str, value: &Value) -> Result<u64, RepositoryError> {
        let matched = self.find_by(column, value).await?;
        if matched.is_empty() {
            return Ok(0);
        }
        let physical_column = self.field_mapping_translator.to_physical(column);
        let sql = build_delete_by(DIALECT, &self.table_name, &physical_column)?;
        let sql =
            PgConvertingRepo::apply_converter_bind_casts(self, sql, &[physical_column.as_str()]);
        let bound = self.apply_to(column, value.clone());
        self.run_query(&sql, &[bound]).await?;
        Ok(matched.len() as u64)
    }
}

#[cfg(test)]
mod where_clause_cast_tests {
    use super::*;
    use crate::repositories::sql_builder::build_select_by_id;
    use crate::repositories::postgres::converter_bind_casts::apply_converter_bind_casts;
    use crate::run::converter_defaults::default_field_converters_for_dialect;
    use crate::run::DialectKind;

    /// Compose the by-id predicate exactly as the runtime does: the synthetic primary key carries the
    /// settings-derived datasource type (`IdType::datasource_type_str`), and postgres appends that
    /// type's `bind_sql_cast` to the `= $1` bind. This is the read/mutate-by-id half of the uuid cast
    /// gap — find/update/delete emit `WHERE id = $1` and postgres rejects `uuid = text` without it.
    fn by_id_predicate_sql(id_type: IdType) -> String {
        let mut column_types: BTreeMap<String, String> = BTreeMap::new();
        if id_type.is_client_supplied() {
            column_types.insert("id".to_string(), id_type.datasource_type_str().to_string());
        }
        let by_type: BTreeMap<String, Arc<dyn TypeFieldConverter>> =
            default_field_converters_for_dialect(&column_types, &DialectKind::Postgres)
                .into_iter()
                .map(|c| (c.datasource_type().to_string(), c))
                .collect();
        let sql = build_select_by_id(Dialect::Postgres, "member", "id").expect("select-by-id sql");
        apply_converter_bind_casts(sql, &["id"], &column_types, &by_type)
    }

    #[test]
    fn uuid_id_predicate_is_cast_to_uuid() {
        let sql = by_id_predicate_sql(IdType::Uuid);
        assert!(
            sql.contains("$1::uuid"),
            "uuid id must cast the bind: {sql}"
        );
    }

    #[test]
    fn integer_id_predicate_is_not_cast() {
        let sql = by_id_predicate_sql(IdType::Integer);
        assert!(
            sql.contains("$1") && !sql.contains("::"),
            "integer id must not cast: {sql}"
        );
    }

    #[test]
    fn string_id_predicate_is_not_cast() {
        let sql = by_id_predicate_sql(IdType::String);
        assert!(
            !sql.contains("::"),
            "string id must not cast (varchar binds directly): {sql}"
        );
    }

    #[test]
    fn uuid_filter_column_predicate_is_cast_to_uuid() {
        let column_types: BTreeMap<String, String> =
            [("conversation_id".to_string(), "uuid".to_string())]
                .into_iter()
                .collect();
        let by_type: BTreeMap<String, Arc<dyn TypeFieldConverter>> =
            default_field_converters_for_dialect(&column_types, &DialectKind::Postgres)
                .into_iter()
                .map(|c| (c.datasource_type().to_string(), c))
                .collect();
        let sql =
            build_select_by_column(Dialect::Postgres, "message", "conversation_id", "id").unwrap();
        let casted = apply_converter_bind_casts(sql, &["conversation_id"], &column_types, &by_type);
        assert!(
            casted.contains("$1::uuid"),
            "uuid filter column (eager-load-by-fk) must cast: {casted}"
        );
    }
}
