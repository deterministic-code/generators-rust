//! Adapts the typed router service traits (CrudService / ReadOnlyService / ByFieldService) onto a
//! composed `DynamicService` stack. A generated service facade forwards each typed method here, so the
//! router's typed calls flow to the runtime's invoke-based decorators (enrichment, eager-write,
//! eager-read, OCC) without the facade reimplementing any of it. Arg/result shapes mirror
//! GenericCrudService::invoke's contract.

use serde_json::{json, Map, Value};

use crate::error::RepositoryError;
use crate::repositories::RowMap;
use crate::services::{DynamicService, ServiceError, IF_MATCH_ARG};

fn to_repo_error(err: ServiceError) -> RepositoryError {
    match err {
        ServiceError::Repository(repo) => repo,
        other => RepositoryError::Other(other.to_string()),
    }
}

fn as_row(value: Value) -> RowMap {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => RowMap::new(),
    }
}

fn as_rows(value: Value) -> Vec<RowMap> {
    match value {
        Value::Array(items) => items.into_iter().map(as_row).collect(),
        _ => Vec::new(),
    }
}

fn as_optional_row(value: Value) -> Option<RowMap> {
    match value {
        Value::Object(map) => Some(map.into_iter().collect()),
        _ => None,
    }
}

fn row_to_value(body: RowMap) -> Value {
    Value::Object(body.into_iter().collect())
}

fn with_if_match(mut args: Map<String, Value>, expected_updated: Option<&str>) -> Value {
    if let Some(token) = expected_updated {
        args.insert(IF_MATCH_ARG.to_string(), Value::String(token.to_string()));
    }
    Value::Object(args)
}

pub async fn find_all(service: &dyn DynamicService) -> Result<Vec<RowMap>, RepositoryError> {
    let value = service
        .invoke("findAll", json!({}))
        .await
        .map_err(to_repo_error)?;
    Ok(as_rows(value))
}

pub async fn find(
    service: &dyn DynamicService,
    id: &Value,
) -> Result<Option<RowMap>, RepositoryError> {
    let value = service
        .invoke("findById", json!({ "id": id }))
        .await
        .map_err(to_repo_error)?;
    Ok(as_optional_row(value))
}

pub async fn find_by(
    service: &dyn DynamicService,
    field: &str,
    value: &Value,
) -> Result<Vec<RowMap>, RepositoryError> {
    let result = service
        .invoke("findBy", json!({ "column": field, "value": value }))
        .await
        .map_err(to_repo_error)?;
    Ok(as_rows(result))
}

pub async fn add(service: &dyn DynamicService, body: RowMap) -> Result<RowMap, RepositoryError> {
    let value = service
        .invoke("create", json!({ "data": row_to_value(body) }))
        .await
        .map_err(to_repo_error)?;
    Ok(as_row(value))
}

pub async fn update(
    service: &dyn DynamicService,
    id: &Value,
    body: RowMap,
    expected_updated: Option<&str>,
) -> Result<Option<RowMap>, RepositoryError> {
    let mut args = Map::new();
    args.insert("id".to_string(), id.clone());
    args.insert("data".to_string(), row_to_value(body));
    let value = service
        .invoke("update", with_if_match(args, expected_updated))
        .await
        .map_err(to_repo_error)?;
    Ok(as_optional_row(value))
}

pub async fn delete(
    service: &dyn DynamicService,
    id: &Value,
    expected_updated: Option<&str>,
) -> Result<bool, RepositoryError> {
    let mut args = Map::new();
    args.insert("id".to_string(), id.clone());
    let value = service
        .invoke("delete", with_if_match(args, expected_updated))
        .await
        .map_err(to_repo_error)?;
    Ok(value
        .get("deleted")
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::inmemory::InMemoryCrudRepository;
    use crate::services::GenericCrudService;
    use std::sync::Arc;

    fn service() -> Arc<dyn DynamicService> {
        Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )))
    }

    #[tokio::test]
    async fn add_then_find_round_trips_through_invoke() {
        let svc = service();
        let created = add(
            svc.as_ref(),
            RowMap::from_iter([("name".to_string(), json!("Ada"))]),
        )
        .await
        .unwrap();
        let id = created.get("id").cloned().unwrap();
        let found = find(svc.as_ref(), &id).await.unwrap().unwrap();
        assert_eq!(found.get("name"), Some(&json!("Ada")));
        assert_eq!(find_all(svc.as_ref()).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn update_and_delete_report_outcomes() {
        let svc = service();
        let created = add(
            svc.as_ref(),
            RowMap::from_iter([("name".to_string(), json!("Ada"))]),
        )
        .await
        .unwrap();
        let id = created.get("id").cloned().unwrap();
        let updated = update(
            svc.as_ref(),
            &id,
            RowMap::from_iter([("name".to_string(), json!("Grace"))]),
            None,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(updated.get("name"), Some(&json!("Grace")));
        assert!(delete(svc.as_ref(), &id, None).await.unwrap());
        assert!(find(svc.as_ref(), &id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn find_by_column_filters() {
        let svc = service();
        add(
            svc.as_ref(),
            RowMap::from_iter([("kind".to_string(), json!("a"))]),
        )
        .await
        .unwrap();
        add(
            svc.as_ref(),
            RowMap::from_iter([("kind".to_string(), json!("b"))]),
        )
        .await
        .unwrap();
        let rows = find_by(svc.as_ref(), "kind", &json!("a")).await.unwrap();
        assert_eq!(rows.len(), 1);
    }
}
