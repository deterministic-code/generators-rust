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
}
