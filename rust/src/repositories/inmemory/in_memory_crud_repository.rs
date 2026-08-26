use async_trait::async_trait;
use serde_json::Value;
use std::sync::Mutex;
use uuid::Uuid;

use crate::error::RepositoryError;
use crate::repositories::crud_repository::CrudRepository;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;
use crate::util::now_iso;

#[derive(Debug)]
pub struct InMemoryCrudRepository {
    state: Mutex<State>,
    has_standard_columns: bool,
    primary_keys: Vec<String>,
}

#[derive(Debug, Default)]
struct State {
    rows: Vec<RowMap>,
    next_id: i64,
}

impl InMemoryCrudRepository {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                rows: Vec::new(),
                next_id: 1,
            }),
            has_standard_columns: true,
            primary_keys: vec!["id".to_string()],
        }
    }

    pub fn with_standard_columns(has_standard_columns: bool) -> Self {
        Self {
            state: Mutex::new(State {
                rows: Vec::new(),
                next_id: 1,
            }),
            has_standard_columns,
            primary_keys: vec!["id".to_string()],
        }
    }

    pub fn with_primary_keys<I, S>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.primary_keys = columns.into_iter().map(Into::into).collect();
        self
    }
}

impl Default for InMemoryCrudRepository {
    fn default() -> Self {
        Self::new()
    }
}

fn row_id(row: &RowMap) -> Option<i64> {
    row.get("id").and_then(|v| v.as_i64())
}

fn row_matches(row: &RowMap, id: &Value, columns: &[String]) -> bool {
    if let Some(obj) = id.as_object() {
        return columns.iter().all(|c| row.get(c) == obj.get(c));
    }
    if columns.len() == 1 {
        return row.get(columns[0].as_str()) == Some(id);
    }
    false
}

#[async_trait]
impl Repository for InMemoryCrudRepository {
    async fn query(&self, _sql: &str, _params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::UnsupportedQuery(
            "InMemory backend does not support raw SQL queries".to_string(),
        ))
    }
}

#[async_trait]
impl CrudRepository for InMemoryCrudRepository {
    fn primary_key_column(&self) -> &str {
        self.primary_keys.first().map(String::as_str).unwrap_or("id")
    }

    fn primary_key_columns(&self) -> Vec<String> {
        self.primary_keys.clone()
    }

    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .rows
            .iter()
            .find(|r| row_matches(r, id, &self.primary_keys))
            .cloned())
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
        if self.primary_keys.len() > 1 {
            for column in &self.primary_keys {
                if !data.contains_key(column) {
                    return Err(RepositoryError::Other(format!(
                        "missing identity key '{column}'"
                    )));
                }
            }
        }
        let mut state = self.state.lock().unwrap();
        let id = state.next_id;
        state.next_id += 1;
        let mut row: RowMap = RowMap::new();
        if self.has_standard_columns {
            let now = now_iso();
            row.insert("uuid".to_string(), Value::from(Uuid::new_v4().to_string()));
            row.insert("created".to_string(), Value::from(now.clone()));
            row.insert("updated".to_string(), Value::from(now));
        }
        for (k, v) in data {
            row.insert(k, v);
        }
        if self.primary_keys.len() <= 1 && self.primary_key_column() == "id" {
            row.insert("id".to_string(), Value::from(id));
        }
        state.rows.push(row.clone());
        Ok(row)
    }

    async fn update(&self, id: &Value, data: RowMap) -> Result<Option<RowMap>, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        let idx = state
            .rows
            .iter()
            .position(|r| row_matches(r, id, &self.primary_keys));
        let Some(idx) = idx else {
            return Ok(None);
        };
        for (k, v) in data {
            state.rows[idx].insert(k, v);
        }
        if self.has_standard_columns {
            state.rows[idx].insert("updated".to_string(), Value::from(now_iso()));
        }
        Ok(Some(state.rows[idx].clone()))
    }

    async fn delete(&self, id: &Value) -> Result<bool, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        let idx = state
            .rows
            .iter()
            .position(|r| row_matches(r, id, &self.primary_keys));
        let Some(idx) = idx else {
            return Ok(false);
        };
        state.rows.remove(idx);
        Ok(true)
    }

    async fn find_in(
        &self,
        column: &str,
        values: &[Value],
    ) -> Result<Vec<RowMap>, RepositoryError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let state = self.state.lock().unwrap();
        let mut rows: Vec<RowMap> = state
            .rows
            .iter()
            .filter(|r| match r.get(column) {
                Some(v) => values.iter().any(|target| target == v),
                None => false,
            })
            .cloned()
            .collect();
        rows.sort_by_key(|r| row_id(r).unwrap_or(0));
        Ok(rows)
    }

    async fn update_by(
        &self,
        column: &str,
        value: &Value,
        data: RowMap,
    ) -> Result<Vec<RowMap>, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        let bump = if self.has_standard_columns {
            Some(now_iso())
        } else {
            None
        };
        let mut updated: Vec<RowMap> = Vec::new();
        for row in state.rows.iter_mut() {
            if row.get(column) != Some(value) {
                continue;
            }
            for (k, v) in &data {
                row.insert(k.clone(), v.clone());
            }
            if let Some(bump) = &bump {
                row.insert("updated".to_string(), Value::from(bump.clone()));
            }
            updated.push(row.clone());
        }
        updated.sort_by_key(|r| row_id(r).unwrap_or(0));
        Ok(updated)
    }

    async fn delete_by(&self, column: &str, value: &Value) -> Result<u64, RepositoryError> {
        let mut state = self.state.lock().unwrap();
        let before = state.rows.len();
        state.rows.retain(|r| r.get(column) != Some(value));
        Ok((before - state.rows.len()) as u64)
    }
}
