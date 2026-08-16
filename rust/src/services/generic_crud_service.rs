use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::dynamic_service::{DynamicService, ServiceError};
use crate::repositories::{CrudRepository, RowMap};

/// Reserved args key carrying the request's `If-Match` value from the dispatch layer to the
/// service, kept out of the write row. The dynamic dispatch and the eager-write service both
/// thread the same key so optimistic concurrency reaches the base repository.
pub(crate) const IF_MATCH_ARG: &str = "__if_match";

pub struct GenericCrudService {
    repository: Arc<dyn CrudRepository>,
    use_optimistic_concurrency: bool,
}

impl GenericCrudService {
    pub fn new(repository: Arc<dyn CrudRepository>) -> Self {
        Self {
            repository,
            use_optimistic_concurrency: false,
        }
    }

    pub fn with_optimistic_concurrency(mut self, enabled: bool) -> Self {
        self.use_optimistic_concurrency = enabled;
        self
    }

    pub fn repository(&self) -> &Arc<dyn CrudRepository> {
        &self.repository
    }

    /// The `expected_updated` an OCC-guarded mutation must pass to the repository: `None` when
    /// concurrency control is off, the trimmed `If-Match` value when on, or a 428 when on and the
    /// caller omitted the header — mirroring the static CRUD router's `require_if_match`.
    fn expected_updated<'a>(&self, args: &'a Value) -> Result<Option<&'a str>, ServiceError> {
        if !self.use_optimistic_concurrency {
            return Ok(None);
        }
        match optional_field(args, IF_MATCH_ARG).and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => Ok(Some(s.trim())),
            _ => Err(ServiceError::PreconditionRequired(
                "If-Match header required for this mutation".to_string(),
            )),
        }
    }
}

#[async_trait]
impl DynamicService for GenericCrudService {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
        match method {
            "findAll" | "find_all" => {
                let rows = self.repository.find_all().await?;
                Ok(rows_to_json(rows))
            }
            "findById" | "find_by_id" | "find" => {
                let id = require_field(&args, "id")?.clone();
                let row = self.repository.find(&id).await?;
                Ok(row.map(row_to_json).unwrap_or(Value::Null))
            }
            "findBy" | "find_by" => {
                let column = require_field(&args, "column")?
                    .as_str()
                    .ok_or_else(|| {
                        ServiceError::InvalidArgs("`column` must be a string".to_string())
                    })?
                    .to_string();
                let value = require_field(&args, "value")?.clone();
                let rows = self.repository.find_by(&column, &value).await?;
                Ok(rows_to_json(rows))
            }
            "create" | "add" => {
                let data = extract_data_row(&args)?;
                let row = self.repository.add(data).await?;
                Ok(row_to_json(row))
            }
            "update" => {
                let id = require_field(&args, "id")?.clone();
                let data = extract_data_row(&args)?;
                let expected = self.expected_updated(&args)?;
                let row = self
                    .repository
                    .update_with_expected(&id, data, expected)
                    .await?;
                Ok(row.map(row_to_json).unwrap_or(Value::Null))
            }
            "delete" => {
                let id = require_field(&args, "id")?.clone();
                let expected = self.expected_updated(&args)?;
                let deleted = self.repository.delete_with_expected(&id, expected).await?;
                Ok(json!({ "deleted": deleted }))
            }
            other => Err(ServiceError::UnknownMethod(other.to_string())),
        }
    }
}

fn require_field<'a>(args: &'a Value, name: &str) -> Result<&'a Value, ServiceError> {
    match args {
        Value::Object(map) => map
            .get(name)
            .ok_or_else(|| ServiceError::InvalidArgs(format!("missing `{}`", name))),
        _ => Err(ServiceError::InvalidArgs(format!(
            "expected object args (looking for `{}`)",
            name
        ))),
    }
}

fn optional_field<'a>(args: &'a Value, name: &str) -> Option<&'a Value> {
    match args {
        Value::Object(map) => map.get(name),
        _ => None,
    }
}

// why `body`/`data` not in this list: they are real column names in some entities (email message.body, attachment.data); the envelope shortcut above already consumes them when they're Object-valued, so any survivor here is a real column.
const RESERVED_ARG_KEYS: &[&str] = &["query", "id", IF_MATCH_ARG];

fn extract_data_row(args: &Value) -> Result<RowMap, ServiceError> {
    // why Value::Object guard: only treat `body`/`data` as envelopes when their value is itself an object; a string/number/null is a real column value.
    if let Some(Value::Object(map)) = optional_field(args, "data") {
        return Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    }
    if let Some(Value::Object(map)) = optional_field(args, "body") {
        return Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    }
    match args {
        Value::Object(obj) => Ok(obj
            .iter()
            .filter(|(k, _)| !RESERVED_ARG_KEYS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()),
        other => Err(ServiceError::InvalidArgs(format!(
            "expected object, got {}",
            type_name_of(other)
        ))),
    }
}

fn row_to_json(row: RowMap) -> Value {
    let mut obj = Map::with_capacity(row.len());
    for (k, v) in row {
        obj.insert(k, v);
    }
    Value::Object(obj)
}

fn rows_to_json(rows: Vec<RowMap>) -> Value {
    Value::Array(rows.into_iter().map(row_to_json).collect())
}

fn type_name_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::inmemory::InMemoryCrudRepository;

    fn build_service() -> GenericCrudService {
        GenericCrudService::new(Arc::new(InMemoryCrudRepository::new()))
    }

    #[tokio::test]
    async fn invoke_create_returns_row_with_id() {
        let svc = build_service();
        let result = svc
            .invoke("create", json!({ "name": "first" }))
            .await
            .unwrap();
        assert_eq!(result["name"], json!("first"));
        assert!(result["id"].is_number());
    }

    #[tokio::test]
    async fn invoke_create_with_data_envelope() {
        let svc = build_service();
        let result = svc
            .invoke("create", json!({ "data": { "name": "a" } }))
            .await
            .unwrap();
        assert_eq!(result["name"], json!("a"));
    }

    #[tokio::test]
    async fn invoke_find_all_returns_inserted_rows_sorted() {
        let svc = build_service();
        svc.invoke("create", json!({ "name": "a" })).await.unwrap();
        svc.invoke("create", json!({ "name": "b" })).await.unwrap();
        let rows = svc.invoke("findAll", json!({})).await.unwrap();
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], json!("a"));
        assert_eq!(arr[1]["name"], json!("b"));
    }

    #[tokio::test]
    async fn invoke_find_by_id_returns_row_or_null() {
        let svc = build_service();
        let created = svc.invoke("create", json!({ "name": "x" })).await.unwrap();
        let id = created["id"].clone();
        let found = svc.invoke("findById", json!({ "id": id })).await.unwrap();
        assert_eq!(found["name"], json!("x"));
        let missing = svc.invoke("findById", json!({ "id": 9999 })).await.unwrap();
        assert!(missing.is_null());
    }

    #[tokio::test]
    async fn invoke_find_by_column_value() {
        let svc = build_service();
        svc.invoke("create", json!({ "name": "a", "tag": "x" }))
            .await
            .unwrap();
        svc.invoke("create", json!({ "name": "b", "tag": "y" }))
            .await
            .unwrap();
        svc.invoke("create", json!({ "name": "c", "tag": "x" }))
            .await
            .unwrap();
        let result = svc
            .invoke("findBy", json!({ "column": "tag", "value": "x" }))
            .await
            .unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[tokio::test]
    async fn invoke_update_merges_fields() {
        let svc = build_service();
        let created = svc
            .invoke("create", json!({ "name": "old", "kept": "yes" }))
            .await
            .unwrap();
        let id = created["id"].clone();
        let updated = svc
            .invoke("update", json!({ "id": id, "data": { "name": "new" } }))
            .await
            .unwrap();
        assert_eq!(updated["name"], json!("new"));
        assert_eq!(updated["kept"], json!("yes"));
    }

    #[tokio::test]
    async fn invoke_update_unknown_id_returns_null() {
        let svc = build_service();
        let result = svc
            .invoke("update", json!({ "id": 99, "data": { "x": 1 } }))
            .await
            .unwrap();
        assert!(result.is_null());
    }

    #[tokio::test]
    async fn invoke_delete_returns_envelope() {
        let svc = build_service();
        let created = svc.invoke("create", json!({ "name": "z" })).await.unwrap();
        let id = created["id"].clone();
        let result = svc
            .invoke("delete", json!({ "id": id.clone() }))
            .await
            .unwrap();
        assert_eq!(result["deleted"], json!(true));
        let second = svc.invoke("delete", json!({ "id": id })).await.unwrap();
        assert_eq!(second["deleted"], json!(false));
    }

    #[tokio::test]
    async fn invoke_unknown_method_errors() {
        let svc = build_service();
        let err = svc.invoke("doSomethingWeird", json!({})).await.unwrap_err();
        match err {
            ServiceError::UnknownMethod(name) => assert_eq!(name, "doSomethingWeird"),
            other => panic!("expected UnknownMethod, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn invoke_create_with_non_object_errors() {
        let svc = build_service();
        let err = svc.invoke("create", json!(42)).await.unwrap_err();
        match err {
            ServiceError::InvalidArgs(msg) => assert!(msg.contains("object")),
            other => panic!("expected InvalidArgs, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn snake_case_aliases_dispatch() {
        let svc = build_service();
        svc.invoke("create", json!({ "x": 1 })).await.unwrap();
        let rows = svc.invoke("find_all", json!({})).await.unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1);
    }

    // Regression: email message.body is a real column; the envelope shortcut must not consume it.
    #[tokio::test]
    async fn invoke_create_with_string_body_column_treats_body_as_data() {
        let svc = build_service();
        let result = svc
            .invoke(
                "create",
                json!({ "subject": "Welcome", "body": "Glad to have you.", "sender": "hr@example.com" }),
            )
            .await
            .unwrap();
        assert_eq!(result["subject"], json!("Welcome"));
        assert_eq!(result["body"], json!("Glad to have you."));
        assert_eq!(result["sender"], json!("hr@example.com"));
        assert!(result["id"].is_number());
    }

    // Regression: attachment.data is a real column; the envelope shortcut must not consume it.
    #[tokio::test]
    async fn invoke_create_with_string_data_column_treats_data_as_value() {
        let svc = build_service();
        let result = svc
            .invoke(
                "create",
                json!({ "filename": "handbook.pdf", "data": "base64payload" }),
            )
            .await
            .unwrap();
        assert_eq!(result["filename"], json!("handbook.pdf"));
        assert_eq!(result["data"], json!("base64payload"));
        assert!(result["id"].is_number());
    }

    #[tokio::test]
    async fn invoke_create_with_body_envelope_still_unwraps_object() {
        let svc = build_service();
        let result = svc
            .invoke("create", json!({ "body": { "name": "from-body" } }))
            .await
            .unwrap();
        assert_eq!(result["name"], json!("from-body"));
    }

    #[tokio::test]
    async fn invoke_update_with_string_body_column_does_not_envelope() {
        let svc = build_service();
        let created = svc
            .invoke("create", json!({ "subject": "old", "body": "first" }))
            .await
            .unwrap();
        let id = created["id"].clone();
        let updated = svc
            .invoke(
                "update",
                json!({ "id": id, "subject": "new", "body": "second" }),
            )
            .await
            .unwrap();
        assert_eq!(updated["subject"], json!("new"));
        assert_eq!(updated["body"], json!("second"));
    }

    fn build_service_occ() -> GenericCrudService {
        GenericCrudService::new(Arc::new(InMemoryCrudRepository::new()))
            .with_optimistic_concurrency(true)
    }

    async fn seed_one(svc: &GenericCrudService) -> Value {
        svc.invoke("create", json!({ "name": "seed" }))
            .await
            .unwrap()["id"]
            .clone()
    }

    #[tokio::test]
    async fn occ_off_update_proceeds_without_if_match() {
        let svc = build_service();
        let id = seed_one(&svc).await;
        let updated = svc
            .invoke("update", json!({ "id": id, "data": { "name": "new" } }))
            .await
            .unwrap();
        assert_eq!(updated["name"], json!("new"));
    }

    #[tokio::test]
    async fn occ_on_update_without_if_match_is_precondition_required() {
        let svc = build_service_occ();
        let id = seed_one(&svc).await;
        let err = svc
            .invoke("update", json!({ "id": id, "data": { "name": "new" } }))
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::PreconditionRequired(_)));
    }

    #[tokio::test]
    async fn occ_on_delete_without_if_match_is_precondition_required() {
        let svc = build_service_occ();
        let id = seed_one(&svc).await;
        let err = svc.invoke("delete", json!({ "id": id })).await.unwrap_err();
        assert!(matches!(err, ServiceError::PreconditionRequired(_)));
    }

    fn update_args_with_if_match(id: Value, token: &str) -> Value {
        let mut args = serde_json::Map::new();
        args.insert("id".to_string(), id);
        args.insert("data".to_string(), json!({ "name": "new" }));
        args.insert(IF_MATCH_ARG.to_string(), json!(token));
        Value::Object(args)
    }

    #[tokio::test]
    async fn occ_on_update_with_if_match_token_proceeds() {
        let svc = build_service_occ();
        let id = seed_one(&svc).await;
        let updated = svc
            .invoke("update", update_args_with_if_match(id, "any-token"))
            .await
            .unwrap();
        assert_eq!(updated["name"], json!("new"));
    }

    #[tokio::test]
    async fn occ_on_whitespace_if_match_is_precondition_required() {
        let svc = build_service_occ();
        let id = seed_one(&svc).await;
        let err = svc
            .invoke("update", update_args_with_if_match(id, "   "))
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::PreconditionRequired(_)));
    }
}
