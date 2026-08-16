use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::dynamic_service::{DynamicService, ServiceError};
use super::eager_child_writing_service::{load_binding_children, EagerWriteChildBindingRuntime};
use crate::repositories::Datasource;

/// The read-side analog of [`super::EagerChildWritingService`]: a `DynamicService` decorator that, on
/// the read methods, loads each declared eager-read relation (`eager_path`) and attaches it to every
/// returned row. Non-read methods delegate to `base` untouched, so this wraps *outside* the enrichment
/// + eager-write chain — the parent row is already enriched by the time children attach. Children are
/// resolved through the same code the eager-write update path uses ([`load_binding_children`]).
pub struct EagerChildReadingService {
    base: Arc<dyn DynamicService>,
    pool_datasource: Arc<dyn Datasource>,
    parent_pk_column: String,
    bindings: Vec<EagerWriteChildBindingRuntime>,
}

impl EagerChildReadingService {
    pub fn new(
        base: Arc<dyn DynamicService>,
        pool_datasource: Arc<dyn Datasource>,
        parent_pk_column: impl Into<String>,
        bindings: Vec<EagerWriteChildBindingRuntime>,
    ) -> Self {
        Self {
            base,
            pool_datasource,
            parent_pk_column: parent_pk_column.into(),
            bindings,
        }
    }

    async fn attach(&self, value: Value) -> Result<Value, ServiceError> {
        match value {
            Value::Array(rows) => {
                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(self.attach_one(row).await?);
                }
                Ok(Value::Array(out))
            }
            other => self.attach_one(other).await,
        }
    }

    async fn attach_one(&self, value: Value) -> Result<Value, ServiceError> {
        let Value::Object(mut row) = value else {
            return Ok(value);
        };
        let Some(parent_id) = row.get(&self.parent_pk_column).cloned() else {
            return Ok(Value::Object(row));
        };
        for binding in &self.bindings {
            let children = load_binding_children(binding, &parent_id, &self.pool_datasource)
                .await
                .map_err(ServiceError::Repository)?;
            row.insert(binding.binding.field_name.clone(), Value::Array(children));
        }
        Ok(Value::Object(row))
    }
}

#[async_trait]
impl DynamicService for EagerChildReadingService {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
        match method {
            "findAll" | "find_all" | "findBy" | "find_by" | "findById" | "find_by_id" | "find"
            | "query" => {
                let result = self.base.invoke(method, args).await?;
                self.attach(result).await
            }
            other => self.base.invoke(other, args).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::eager_write::{BindingKind, EagerWriteChildBinding};
    use crate::repositories::sqlite::{
        SqliteCrudRepository, SqliteDatasource, SqliteDatasourceOptions,
    };
    use crate::services::eager_child_writing_service::ServiceFactory;
    use crate::services::GenericCrudService;
    use serde_json::json;

    async fn open_sqlite() -> Arc<SqliteDatasource> {
        let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
        ds.open().await.unwrap();
        ds.query(
            "CREATE TABLE contact (id INTEGER PRIMARY KEY AUTOINCREMENT, first_name TEXT)",
            &[],
        )
        .await
        .unwrap();
        ds.query(
            "CREATE TABLE address (id INTEGER PRIMARY KEY AUTOINCREMENT, contact_id INTEGER, line1 TEXT)",
            &[],
        )
        .await
        .unwrap();
        Arc::new(ds)
    }

    fn factory(table: &str) -> ServiceFactory {
        let table = table.to_string();
        Arc::new(move |ds: Arc<dyn Datasource>| {
            let repo = Arc::new(SqliteCrudRepository::new(ds, &table)?);
            let svc: Arc<dyn DynamicService> = Arc::new(GenericCrudService::new(repo));
            Ok(svc)
        })
    }

    fn addresses_binding() -> EagerWriteChildBindingRuntime {
        EagerWriteChildBindingRuntime {
            binding: EagerWriteChildBinding {
                kind: BindingKind::DirectFk {
                    fk_column: "contact_id".to_string(),
                },
                field_name: "addresses".to_string(),
                child_table: "address".to_string(),
                children: Vec::new(),
            },
            child_service_factory: factory("address"),
            junction_repo_factory: None,
            children: Vec::new(),
        }
    }

    async fn reading_service(ds: Arc<SqliteDatasource>) -> EagerChildReadingService {
        let pool: Arc<dyn Datasource> = ds.clone();
        let base = factory("contact")(pool.clone()).unwrap();
        EagerChildReadingService::new(base, pool, "id", vec![addresses_binding()])
    }

    #[tokio::test]
    async fn find_by_id_attaches_non_empty_children() {
        let ds = open_sqlite().await;
        let pool: Arc<dyn Datasource> = ds.clone();
        let contact = factory("contact")(pool.clone())
            .unwrap()
            .invoke("create", json!({ "first_name": "Ada" }))
            .await
            .unwrap();
        let id = contact["id"].clone();
        factory("address")(pool.clone())
            .unwrap()
            .invoke(
                "create",
                json!({ "contact_id": id.clone(), "line1": "1 Main St" }),
            )
            .await
            .unwrap();

        let svc = reading_service(ds).await;
        let row = svc.invoke("findById", json!({ "id": id })).await.unwrap();
        let addresses = row["addresses"].as_array().expect("addresses");
        assert_eq!(addresses.len(), 1);
        assert_eq!(addresses[0]["line1"], json!("1 Main St"));
    }

    #[tokio::test]
    async fn find_all_gives_childless_parents_an_empty_array_not_a_missing_key() {
        let ds = open_sqlite().await;
        let pool: Arc<dyn Datasource> = ds.clone();
        factory("contact")(pool.clone())
            .unwrap()
            .invoke("create", json!({ "first_name": "Grace" }))
            .await
            .unwrap();

        let svc = reading_service(ds).await;
        let rows = svc.invoke("findAll", json!({})).await.unwrap();
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert!(arr[0].get("addresses").is_some());
        assert_eq!(arr[0]["addresses"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn write_methods_delegate_without_attaching() {
        let ds = open_sqlite().await;
        let svc = reading_service(ds).await;
        let created = svc
            .invoke("create", json!({ "first_name": "Ada" }))
            .await
            .unwrap();
        assert_eq!(created["first_name"], json!("Ada"));
        assert!(created.get("addresses").is_none());
    }
}
