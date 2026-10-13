use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::dynamic_service::{DynamicService, ServiceError, IF_MATCH_ARG};
use crate::error::RepositoryError;
use crate::routes::{ByFieldService, CrudService, ReadOnlyService};
use crate::types::RowMap;

/// In-process DynamicService used by generated stubs and crate tests after the old ORM was removed.
/// Generated Datasources repositories replace this for real persistence.
pub struct InMemoryDynamicService {
    state: Mutex<Store>,
}

#[derive(Default)]
struct Store {
    rows: Vec<RowMap>,
    next_id: i64,
}

impl InMemoryDynamicService {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(Store {
                rows: Vec::new(),
                next_id: 1,
            }),
        }
    }

    fn row_id(row: &RowMap) -> Option<Value> {
        row.get("id").cloned()
    }

    fn ids_equal(left: &Value, right: &Value) -> bool {
        left == right || left.to_string() == right.to_string()
    }
}

impl Default for InMemoryDynamicService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DynamicService for InMemoryDynamicService {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
        match method {
            "findAll" | "find_all" => Ok(rows_to_json(CrudService::find_all(self).await?)),
            "findById" | "find_by_id" | "find" => {
                let id = require_field(&args, "id")?.clone();
                Ok(CrudService::find(self, &id)
                    .await?
                    .map(row_to_json)
                    .unwrap_or(Value::Null))
            }
            "findBy" | "find_by" => {
                let column = require_field(&args, "column")?
                    .as_str()
                    .ok_or_else(|| ServiceError::InvalidArgs("`column` must be a string".into()))?
                    .to_string();
                let value = require_field(&args, "value")?.clone();
                Ok(rows_to_json(self.find_by(&column, &value).await?))
            }
            "create" | "add" => {
                let data = extract_data_row(&args)?;
                Ok(row_to_json(self.add(data).await?))
            }
            "update" => {
                let id = require_field(&args, "id")?.clone();
                let data = extract_data_row(&args)?;
                Ok(self
                    .update(&id, data, None)
                    .await?
                    .map(row_to_json)
                    .unwrap_or(Value::Null))
            }
            "delete" => {
                let id = require_field(&args, "id")?.clone();
                Ok(json!({ "deleted": self.delete(&id, None).await? }))
            }
            other => Err(ServiceError::UnknownMethod(other.to_string())),
        }
    }
}

#[async_trait]
impl CrudService for InMemoryDynamicService {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        Ok(self.state.lock().expect("in-memory store").rows.clone())
    }

    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let store = self.state.lock().expect("in-memory store");
        Ok(store
            .rows
            .iter()
            .find(|row| Self::row_id(row).is_some_and(|row_id| Self::ids_equal(&row_id, id)))
            .cloned())
    }

    async fn add(&self, mut body: RowMap) -> Result<RowMap, RepositoryError> {
        let mut store = self.state.lock().expect("in-memory store");
        if !body.contains_key("id") {
            body.insert("id".to_string(), json!(store.next_id));
            store.next_id += 1;
        }
        store.rows.push(body.clone());
        Ok(body)
    }

    async fn update(
        &self,
        id: &Value,
        body: RowMap,
        _expected_updated: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        let mut store = self.state.lock().expect("in-memory store");
        let Some(index) = store
            .rows
            .iter()
            .position(|row| Self::row_id(row).is_some_and(|row_id| Self::ids_equal(&row_id, id)))
        else {
            return Ok(None);
        };
        let mut merged = store.rows[index].clone();
        for (key, value) in body {
            merged.insert(key, value);
        }
        merged.insert("id".to_string(), id.clone());
        store.rows[index] = merged.clone();
        Ok(Some(merged))
    }

    async fn delete(
        &self,
        id: &Value,
        _expected_updated: Option<&str>,
    ) -> Result<bool, RepositoryError> {
        let mut store = self.state.lock().expect("in-memory store");
        let before = store.rows.len();
        store
            .rows
            .retain(|row| !Self::row_id(row).is_some_and(|row_id| Self::ids_equal(&row_id, id)));
        Ok(store.rows.len() < before)
    }
}

#[async_trait]
impl ReadOnlyService for InMemoryDynamicService {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        CrudService::find_all(self).await
    }

    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        CrudService::find(self, id).await
    }
}

#[async_trait]
impl ByFieldService for InMemoryDynamicService {
    async fn find_by(&self, field: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        let store = self.state.lock().expect("in-memory store");
        Ok(store
            .rows
            .iter()
            .filter(|row| row.get(field).is_some_and(|cell| cell == value))
            .cloned()
            .collect())
    }
}

fn require_field<'a>(args: &'a Value, name: &str) -> Result<&'a Value, ServiceError> {
    match args {
        Value::Object(map) => map
            .get(name)
            .ok_or_else(|| ServiceError::InvalidArgs(format!("missing `{name}`"))),
        _ => Err(ServiceError::InvalidArgs(format!(
            "expected object args (looking for `{name}`)"
        ))),
    }
}

fn optional_field<'a>(args: &'a Value, name: &str) -> Option<&'a Value> {
    match args {
        Value::Object(map) => map.get(name),
        _ => None,
    }
}

const RESERVED_ARG_KEYS: &[&str] = &["query", "id", IF_MATCH_ARG];

fn extract_data_row(args: &Value) -> Result<RowMap, ServiceError> {
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
    Value::Object(row.into_iter().collect::<Map<_, _>>())
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
