use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::RepositoryError;
use crate::repositories::{CrudRepository, RowMap};
use crate::routes::crud::{EnrichItemFn, EnrichItemsFn};
use crate::routes::helpers::{
    internal_error, normalize_base, not_found, parse_id, row_map_to_value, run_enrich_item_hook,
    run_enrich_items_hook, validation_error, IdType,
};

#[async_trait]
pub trait ReadOnlyService: Send + Sync + 'static {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError>;
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError>;
}

#[async_trait]
impl<R> ReadOnlyService for R
where
    R: CrudRepository + 'static,
{
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        CrudRepository::find_all(self).await
    }
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        CrudRepository::find(self, id).await
    }
}

pub struct ReadOnlyRouterConfig {
    pub service: Arc<dyn ReadOnlyService>,
    pub entity_name: String,
    pub base_path: String,
    pub id_type: IdType,
    pub enrich_items: Option<EnrichItemsFn>,
    pub enrich_item: Option<EnrichItemFn>,
}

pub fn create_read_only_router(cfg: ReadOnlyRouterConfig) -> Router {
    let entity_name = cfg.entity_name;
    let base = normalize_base(&cfg.base_path);
    let list_path = base.clone();
    let id_path = format!("{}/{{id}}", base);
    Router::new()
        .route(&list_path, get(list_handler))
        .route(&id_path, get(get_by_id_handler))
        .with_state(RouterState {
            service: cfg.service,
            entity_name: Arc::new(entity_name),
            id_type: cfg.id_type,
            enrich_items: cfg.enrich_items,
            enrich_item: cfg.enrich_item,
        })
}

#[derive(Clone)]
struct RouterState {
    service: Arc<dyn ReadOnlyService>,
    entity_name: Arc<String>,
    id_type: IdType,
    enrich_items: Option<EnrichItemsFn>,
    enrich_item: Option<EnrichItemFn>,
}

async fn run_enrich_items(state: &RouterState, rows: Vec<RowMap>) -> Result<Vec<RowMap>, Response> {
    run_enrich_items_hook(state.enrich_items.as_ref(), rows).await
}

async fn run_enrich_item(state: &RouterState, row: RowMap) -> Result<RowMap, Response> {
    run_enrich_item_hook(state.enrich_item.as_ref(), row).await
}

async fn list_handler(State(state): State<RouterState>) -> impl IntoResponse {
    let rows = match state.service.find_all().await {
        Ok(rows) => rows,
        Err(err) => return internal_error(err.to_string()),
    };
    let rows = match run_enrich_items(&state, rows).await {
        Ok(rows) => rows,
        Err(resp) => return resp,
    };
    let items: Vec<Value> = rows.into_iter().map(row_map_to_value).collect();
    (StatusCode::OK, Json(json!({ "items": items }))).into_response()
}

async fn get_by_id_handler(
    State(state): State<RouterState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let id_value = match parse_id(state.id_type, &id) {
        Ok(v) => v,
        Err(msg) => return validation_error(msg),
    };
    let row = match state.service.find(&id_value).await {
        Ok(Some(row)) => row,
        Ok(None) => return not_found(&state.entity_name, &id),
        Err(err) => return internal_error(err.to_string()),
    };
    let row = match run_enrich_item(&state, row).await {
        Ok(row) => row,
        Err(resp) => return resp,
    };
    (StatusCode::OK, Json(row_map_to_value(row))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(
        enrich_items: Option<EnrichItemsFn>,
        enrich_item: Option<EnrichItemFn>,
    ) -> RouterState {
        RouterState {
            service: Arc::new(NoopService),
            entity_name: Arc::new("User".to_string()),
            id_type: IdType::Integer,
            enrich_items,
            enrich_item,
        }
    }

    struct NoopService;

    #[async_trait]
    impl ReadOnlyService for NoopService {
        async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
            Ok(Vec::new())
        }
        async fn find(&self, _id: &Value) -> Result<Option<RowMap>, RepositoryError> {
            Ok(None)
        }
    }

    fn row(k: &str, v: Value) -> RowMap {
        let mut r = RowMap::new();
        r.insert(k.to_string(), v);
        r
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_enrich_items_is_passthrough_when_hook_is_none() {
        let state = state_with(None, None);
        let rows = vec![row("id", Value::from(1i64))];
        let out = run_enrich_items(&state, rows.clone()).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].get("id").unwrap(), &Value::from(1i64));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_enrich_items_applies_hook_when_present() {
        let hook: EnrichItemsFn = Arc::new(|rows: Vec<RowMap>| {
            Box::pin(async move {
                let out: Vec<RowMap> = rows
                    .into_iter()
                    .map(|mut r| {
                        r.insert("touched".to_string(), Value::Bool(true));
                        r
                    })
                    .collect();
                Ok(out)
            })
        });
        let state = state_with(Some(hook), None);
        let out = run_enrich_items(&state, vec![row("id", Value::from(1i64))])
            .await
            .unwrap();
        assert_eq!(out[0].get("touched").unwrap(), &Value::Bool(true));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_enrich_items_maps_hook_error_to_500_response() {
        let hook: EnrichItemsFn = Arc::new(|_rows| {
            Box::pin(async move {
                Err(crate::routes::crud::RouteHookError::Repository(
                    RepositoryError::UnsupportedQuery("items enrichment blew up".to_string()),
                ))
            })
        });
        let state = state_with(Some(hook), None);
        let resp = run_enrich_items(&state, Vec::new()).await.unwrap_err();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_enrich_item_is_passthrough_when_hook_is_none() {
        let state = state_with(None, None);
        let out = run_enrich_item(&state, row("id", Value::from(1i64)))
            .await
            .unwrap();
        assert_eq!(out.get("id").unwrap(), &Value::from(1i64));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_enrich_item_applies_hook_when_present() {
        let hook: EnrichItemFn = Arc::new(|mut row: RowMap| {
            Box::pin(async move {
                row.insert("touched".to_string(), Value::Bool(true));
                Ok(row)
            })
        });
        let state = state_with(None, Some(hook));
        let out = run_enrich_item(&state, row("id", Value::from(1i64)))
            .await
            .unwrap();
        assert_eq!(out.get("touched").unwrap(), &Value::Bool(true));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_enrich_item_maps_hook_error_to_500_response() {
        let hook: EnrichItemFn = Arc::new(|_row| {
            Box::pin(async move {
                Err(crate::routes::crud::RouteHookError::Repository(
                    RepositoryError::UnsupportedQuery("item enrichment blew up".to_string()),
                ))
            })
        });
        let state = state_with(None, Some(hook));
        let resp = run_enrich_item(&state, RowMap::new()).await.unwrap_err();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    fn router_with(id_type: IdType) -> Router {
        create_read_only_router(ReadOnlyRouterConfig {
            service: Arc::new(NoopService),
            entity_name: "notification_type".to_string(),
            base_path: "/api/notification-types".to_string(),
            id_type,
            enrich_items: None,
            enrich_item: None,
        })
    }

    async fn get_status(id_type: IdType, uri: &str) -> StatusCode {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        router_with(id_type)
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("build get request"),
            )
            .await
            .expect("router get")
            .status()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn string_pk_missing_key_is_404_not_400() {
        let status = get_status(IdType::String, "/api/notification-types/missing-key").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn integer_pk_non_numeric_key_is_400() {
        let status = get_status(IdType::Integer, "/api/notification-types/missing-key").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn integer_pk_absent_numeric_key_is_404() {
        let status = get_status(IdType::Integer, "/api/notification-types/999").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uuid_pk_valid_key_is_404_not_400() {
        let status = get_status(
            IdType::Uuid,
            "/api/notification-types/00000000-0000-0000-0000-000000000007",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uuid_pk_non_uuid_key_is_400() {
        let status = get_status(IdType::Uuid, "/api/notification-types/7").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
