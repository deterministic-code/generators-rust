use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::RepositoryError;
use crate::mappings::field_mapping_translator::FieldMappingTranslator;
use crate::mappings::postgres_date_time_converter::PostgresDateTimeConverter;
use crate::mappings::postgres_uuid_converter::PostgresUuidConverter;
use crate::mappings::type_field_converter::TypeFieldConverter;
use crate::repositories::crud_repository::CrudRepository;
use crate::repositories::datasource_middleware::run_query_with_middlewares;
use crate::repositories::datasource_middleware::{DataSourceMiddleware, DatabaseBackend};
use crate::repositories::postgres::pg_converting_repo::PgConvertingRepo;
use crate::repositories::postgres::postgres_datasource::PostgresDatasource;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::repositories::sql_builder::{
    build_delete, build_insert, build_select_all, build_select_by_column, build_select_by_id,
    build_update, Dialect,
};
use crate::repositories::standard_crud_repository::StandardCrudRepository;
use crate::sql_identifier::validate_identifier;

const DIALECT: Dialect = Dialect::Postgres;
const BACKEND: DatabaseBackend = DatabaseBackend::Postgres;

pub struct PostgresStandardRepository {
    datasource: Arc<PostgresDatasource>,
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

impl PostgresStandardRepository {
    pub fn new(
        datasource: Arc<PostgresDatasource>,
        table_name: impl Into<String>,
    ) -> Result<Self, RepositoryError> {
        let table_name = table_name.into();
        DIALECT.quote(&table_name)?;
        let entity_name = table_name.clone();
        let entity_name_plural = format!("{}s", table_name);
        let passthrough_translator = Arc::new(FieldMappingTranslator::new(None)?);
        let (column_datasource_types, converters_by_type) = default_audit_column_typing();
        Ok(Self {
            datasource,
            table_name,
            entity_name,
            entity_name_plural,
            middlewares: Vec::new(),
            field_mapping_translator: passthrough_translator,
            converters_by_type,
            column_datasource_types,
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

    // why extend (vs replace): `new()` seeds audit-column typing (uuid/created/
    // updated → ::uuid/::timestamptz). Codegen layers entity-specific datetime/
    // uuid columns on top via this method — replacing would clobber the audit
    // defaults and every INSERT 500s on the audit binds.
    pub fn with_converters(mut self, converters: Vec<Arc<dyn TypeFieldConverter>>) -> Self {
        for c in converters {
            self.converters_by_type
                .insert(c.datasource_type().to_string(), c);
        }
        self
    }

    pub fn with_column_datasource_types(
        mut self,
        column_datasource_types: BTreeMap<String, String>,
    ) -> Self {
        self.column_datasource_types.extend(column_datasource_types);
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

    fn build_call_sql(proc_name: &str, params_len: usize) -> String {
        let placeholders: Vec<String> = (1..=params_len).map(|i| format!("${}", i)).collect();
        format!("SELECT * FROM {}({})", proc_name, placeholders.join(", "))
    }

    async fn invoke_returning_id(
        &self,
        proc_name: &str,
        params: &[Value],
    ) -> Result<i64, RepositoryError> {
        let sql = Self::build_call_sql(proc_name, params.len());
        let rows = self.run_query(&sql, params).await?;
        let head = rows.into_iter().next().ok_or_else(|| {
            RepositoryError::Other(format!("{} returned no row for scalar id", proc_name))
        })?;
        head.get(proc_name).and_then(|v| v.as_i64()).ok_or_else(|| {
            RepositoryError::Other(format!("{} did not return scalar id", proc_name))
        })
    }

    async fn invoke_returning_affected(
        &self,
        proc_name: &str,
        params: &[Value],
    ) -> Result<i64, RepositoryError> {
        let sql = Self::build_call_sql(proc_name, params.len());
        let rows = self.run_query(&sql, params).await?;
        let head = rows.into_iter().next().ok_or_else(|| {
            RepositoryError::Other(format!("{} returned no row for rows-affected", proc_name))
        })?;
        head.get(proc_name).and_then(|v| v.as_i64()).ok_or_else(|| {
            RepositoryError::Other(format!("{} did not return rows-affected", proc_name))
        })
    }

    async fn invoke_returning_rows(
        &self,
        proc_name: &str,
        params: &[Value],
    ) -> Result<Vec<RowMap>, RepositoryError> {
        let sql = Self::build_call_sql(proc_name, params.len());
        self.run_query(&sql, params).await
    }

    pub async fn update_with_expected(
        &self,
        id: i64,
        data: RowMap,
        expected_updated: &str,
    ) -> Result<Option<RowMap>, RepositoryError> {
        let id_value = Value::from(id);
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
        self.find(&id_value).await
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

// why: the standard repo owns `uuid`/`created`/`updated`; register their datasource types so converter-driven bind casts (e.g. ::uuid, ::timestamptz) apply without codegen wiring.
fn default_audit_column_typing() -> (
    BTreeMap<String, String>,
    BTreeMap<String, Arc<dyn TypeFieldConverter>>,
) {
    let mut columns: BTreeMap<String, String> = BTreeMap::new();
    columns.insert("uuid".to_string(), "uuid".to_string());
    columns.insert("created".to_string(), "datetime".to_string());
    columns.insert("updated".to_string(), "datetime".to_string());
    let mut converters: BTreeMap<String, Arc<dyn TypeFieldConverter>> = BTreeMap::new();
    converters.insert(
        "datetime".to_string(),
        Arc::new(PostgresDateTimeConverter) as Arc<dyn TypeFieldConverter>,
    );
    converters.insert(
        "uuid".to_string(),
        Arc::new(PostgresUuidConverter) as Arc<dyn TypeFieldConverter>,
    );
    (columns, converters)
}

impl PgConvertingRepo for PostgresStandardRepository {
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
impl Repository for PostgresStandardRepository {
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        self.run_query(sql, params).await
    }
}

#[async_trait]
impl CrudRepository for PostgresStandardRepository {
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
        let sql = build_select_by_id(DIALECT, &self.table_name, "id")?;
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
        let sql = build_select_all(DIALECT, &self.table_name, "id")?;
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
        let sql = build_select_by_column(DIALECT, &self.table_name, &physical_column, "id")?;
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

        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let values: Vec<Value> = prepared.values().cloned().collect();
        let sql = build_insert(DIALECT, &self.table_name, &columns, "id")?;
        let sql = PgConvertingRepo::apply_converter_bind_casts(self, sql, &columns);
        let rows = self.run_query(&sql, &values).await?;
        rows.into_iter()
            .next()
            .map(|r| self.apply_from_and_translate(r))
            .ok_or(RepositoryError::InsertedRowMissing(0))
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

        let prepared = self.prepare_write_row(data);
        let columns: Vec<&str> = prepared.keys().map(String::as_str).collect();
        let mut values: Vec<Value> = prepared.values().cloned().collect();
        values.push(id.clone());
        let sql = build_update(DIALECT, &self.table_name, &columns, "id")?;
        let sql = PgConvertingRepo::apply_converter_bind_casts(self, sql, &columns);
        let rows = self.run_query(&sql, &values).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|r| self.apply_from_and_translate(r)))
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
        let sql = build_delete(DIALECT, &self.table_name, "id")?;
        let rows = self.run_query(&sql, std::slice::from_ref(id)).await?;
        Ok(!rows.is_empty())
    }
}

impl StandardCrudRepository for PostgresStandardRepository {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_audit_typing_registers_uuid_and_audit_timestamps() {
        let (col_types, convs) = default_audit_column_typing();
        assert_eq!(col_types.get("uuid").map(String::as_str), Some("uuid"));
        assert_eq!(
            col_types.get("created").map(String::as_str),
            Some("datetime")
        );
        assert_eq!(
            col_types.get("updated").map(String::as_str),
            Some("datetime")
        );
        let dt = convs
            .get("datetime")
            .expect("datetime converter registered");
        assert_eq!(dt.bind_sql_cast(), Some("::timestamptz"));
        let u = convs.get("uuid").expect("uuid converter registered");
        assert_eq!(u.bind_sql_cast(), Some("::uuid"));
    }

    fn make_repo() -> PostgresStandardRepository {
        let ds = Arc::new(PostgresDatasource::new(
            crate::repositories::postgres::postgres_datasource::PostgresDatasourceOptions {
                url: "postgres://unused".to_string(),
                max_connections: 1,
            },
        ));
        PostgresStandardRepository::new(ds, "messages").unwrap()
    }

    #[test]
    fn with_column_datasource_types_extends_audit_defaults() {
        let repo = make_repo().with_column_datasource_types({
            let mut m = BTreeMap::new();
            m.insert("received_at".to_string(), "datetime".to_string());
            m
        });
        let types = PgConvertingRepo::column_datasource_types(&repo);
        assert_eq!(types.get("uuid").map(String::as_str), Some("uuid"));
        assert_eq!(types.get("created").map(String::as_str), Some("datetime"));
        assert_eq!(types.get("updated").map(String::as_str), Some("datetime"));
        assert_eq!(
            types.get("received_at").map(String::as_str),
            Some("datetime")
        );
    }

    #[test]
    fn with_converters_extends_audit_defaults() {
        let repo = make_repo().with_converters(vec![
            Arc::new(PostgresDateTimeConverter) as Arc<dyn TypeFieldConverter>
        ]);
        let convs = PgConvertingRepo::converters_by_type(&repo);
        assert!(
            convs.get("datetime").is_some(),
            "datetime converter must remain"
        );
        assert!(convs.get("uuid").is_some(), "uuid converter must remain");
    }

    #[test]
    fn apply_converter_bind_casts_casts_user_datetime_after_extend() {
        let repo = make_repo().with_column_datasource_types({
            let mut m = BTreeMap::new();
            m.insert("received_at".to_string(), "datetime".to_string());
            m
        });
        let sql = PgConvertingRepo::apply_converter_bind_casts(
            &repo,
            "INSERT INTO messages (\"received_at\", \"uuid\") VALUES ($1, $2)".to_string(),
            &["received_at", "uuid"],
        );
        assert!(
            sql.contains("$1::timestamptz"),
            "user datetime column should get ::timestamptz: {}",
            sql
        );
        assert!(
            sql.contains("$2::uuid"),
            "audit uuid column should still get ::uuid: {}",
            sql
        );
    }
}
