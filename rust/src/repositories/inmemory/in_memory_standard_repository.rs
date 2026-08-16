use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use std::sync::Mutex;
use uuid::Uuid;

use crate::error::RepositoryError;
use crate::repositories::crud_repository::CrudRepository;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::repositories::standard_crud_repository::StandardCrudRepository;

#[derive(Debug, Default)]
pub struct InMemoryStandardRepository {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    rows: Vec<RowMap>,
    next_id: i64,
}

impl InMemoryStandardRepository {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                rows: Vec::new(),
                next_id: 1,
            }),
        }
    }
}

fn row_id(row: &RowMap) -> Option<i64> {
    row.get("id").and_then(|v| v.as_i64())
}

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[async_trait]
impl Repository for InMemoryStandardRepository {
    async fn query(&self, _sql: &str, _params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::UnsupportedQuery(
            "InMemory backend does not support raw SQL queries".to_string(),
        ))
    }
}

#[async_trait]
impl CrudRepository for InMemoryStandardRepository {
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let target = id.as_i64();
        let state = self.state.lock().unwrap();
        Ok(state.rows.iter().find(|r| row_id(r) == target).cloned())
    }

    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        let state = self.state.lock().unwrap();
        let mut rows = state.rows.clone();
        rows.sort_by_key(|r| row_id(r).unwrap_or(0));
        Ok(rows)
    }

    async fn find_by(&self, column: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError> {
        let state = self.state.lock().unwrap();
        let mut rows: Vec<RowMap> = state
            .rows
            .iter()
            .filter(|r| r.get(column) == Some(value))
            .cloned()
            .collect();
        rows.sort_by_key(|r| row_id(r).unwrap_or(0));
        Ok(rows)
    }

    async fn add(&self, data: RowMap) -> Result<RowMap, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        let id = state.next_id;
        state.next_id += 1;
        let now = now_iso();
        let mut row = data;
        row.insert("id".to_string(), Value::from(id));
        row.insert("uuid".to_string(), Value::from(Uuid::new_v4().to_string()));
        row.insert("created".to_string(), Value::from(now.clone()));
        row.insert("updated".to_string(), Value::from(now));
        state.rows.push(row.clone());
        Ok(row)
    }

    async fn update(&self, id: &Value, data: RowMap) -> Result<Option<RowMap>, RepositoryError> {
        let target = id.as_i64();
        let mut state = self.state.lock().unwrap();
        let idx = state.rows.iter().position(|r| row_id(r) == target);
        let Some(idx) = idx else {
            return Ok(None);
        };
        for (k, v) in data {
            state.rows[idx].insert(k, v);
        }
        state.rows[idx].insert("updated".to_string(), Value::from(now_iso()));
        Ok(Some(state.rows[idx].clone()))
    }

    async fn delete(&self, id: &Value) -> Result<bool, RepositoryError> {
        let target = id.as_i64();
        let mut state = self.state.lock().unwrap();
        let idx = state.rows.iter().position(|r| row_id(r) == target);
        let Some(idx) = idx else {
            return Ok(false);
        };
        state.rows.remove(idx);
        Ok(true)
    }
}

impl StandardCrudRepository for InMemoryStandardRepository {}
