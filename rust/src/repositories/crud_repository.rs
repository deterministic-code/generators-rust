use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use crate::error::RepositoryError;
use crate::id_type::IdType;
use crate::repositories::repository::Repository;
use crate::repositories::row_map::RowMap;

/// Fill the implicit `id` primary key with a fresh uuid when the datasource id_type is uuid and
/// the caller omitted it. A uuid PK has no DB auto-increment to read back, so the app owns
/// generation (all dialects agree on the value). Custom or caller-supplied keys are left as-is.
pub(crate) fn fill_uuid_primary_key(data: &mut RowMap, id_type: IdType, logical_primary_key: &str) {
    if id_type.is_uuid() && logical_primary_key == "id" && !data.contains_key(logical_primary_key) {
        data.insert(
            logical_primary_key.to_string(),
            Value::from(Uuid::new_v4().hyphenated().to_string()),
        );
    }
}

#[async_trait]
pub trait CrudRepository: Repository {
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError>;
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError>;
    async fn find_by(&self, column: &str, value: &Value) -> Result<Vec<RowMap>, RepositoryError>;
    async fn find_in(
        &self,
        _column: &str,
        _values: &[Value],
    ) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "find_in not implemented for this backend",
        ))
    }
    async fn add(&self, data: RowMap) -> Result<RowMap, RepositoryError>;
    async fn update(&self, id: &Value, data: RowMap) -> Result<Option<RowMap>, RepositoryError>;
    async fn update_with_expected(
        &self,
        id: &Value,
        data: RowMap,
        _expected_updated: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        self.update(id, data).await
    }
    async fn update_by(
        &self,
        _column: &str,
        _value: &Value,
        _data: RowMap,
    ) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "update_by not implemented for this backend",
        ))
    }
    async fn delete(&self, id: &Value) -> Result<bool, RepositoryError>;
    async fn delete_with_expected(
        &self,
        id: &Value,
        _expected_updated: Option<&str>,
    ) -> Result<bool, RepositoryError> {
        self.delete(id).await
    }
    async fn delete_by(&self, _column: &str, _value: &Value) -> Result<u64, RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "delete_by not implemented for this backend",
        ))
    }
    fn primary_key_column(&self) -> &str {
        "id"
    }

    fn primary_key_columns(&self) -> Vec<String> {
        vec![self.primary_key_column().to_string()]
    }
}

/// Address a row by the full identity: a scalar for one column, or a named object.
pub fn identity_from_row(row: &RowMap, columns: &[String]) -> Value {
    if columns.len() <= 1 {
        let column = columns.first().map(String::as_str).unwrap_or("id");
        return row.get(column).cloned().unwrap_or(Value::Null);
    }
    let mut obj = serde_json::Map::new();
    for column in columns {
        if let Some(value) = row.get(column) {
            obj.insert(column.clone(), value.clone());
        }
    }
    Value::Object(obj)
}

/// Bind a scalar id or a JSON object of identity columns, in `columns` order.
pub fn bind_identity(id: &Value, columns: &[String]) -> Result<Vec<Value>, RepositoryError> {
    if let Some(obj) = id.as_object() {
        let mut out = Vec::with_capacity(columns.len());
        for column in columns {
            let value = obj.get(column).cloned().ok_or_else(|| {
                RepositoryError::Other(format!("missing identity key '{column}'"))
            })?;
            out.push(value);
        }
        return Ok(out);
    }
    if columns.len() == 1 {
        return Ok(vec![id.clone()]);
    }
    Err(RepositoryError::Other(
        "composite identity requires a named object".into(),
    ))
}
