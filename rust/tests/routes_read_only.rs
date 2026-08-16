use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use deterministic::repositories::inmemory::InMemoryCrudRepository;
use deterministic::repositories::{CrudRepository, RowMap};
use deterministic::routes::{create_read_only_router, IdType, ReadOnlyRouterConfig};

async fn seed(repo: &InMemoryCrudRepository, count: usize) {
    for i in 1..=count {
        let mut row = RowMap::new();
        row.insert("name".to_string(), Value::String(format!("user-{}", i)));
        repo.add(row).await.unwrap();
    }
}

fn generated_app(repo: Arc<InMemoryCrudRepository>) -> Router {
    create_read_only_router(ReadOnlyRouterConfig {
        service: repo,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        id_type: IdType::Integer,
        enrich_items: None,
        enrich_item: None,
    })
}

fn custom_router() -> Router {
    Router::new().route(
        "/api/custom-thing",
        get(|| async { axum::Json(json!({ "custom": "yes" })) }),
    )
}

fn merged_app(repo: Arc<InMemoryCrudRepository>) -> Router {
    Router::new()
        .merge(generated_app(repo))
        .merge(custom_router())
}

async fn json_get(app: Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

#[tokio::test]
async fn generated_list_returns_empty_items_when_repo_has_no_rows() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    let (status, body) = json_get(merged_app(repo), "/api/users").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "items": [] }));
}

#[tokio::test]
async fn generated_list_returns_one_item() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 1).await;
    let (status, body) = json_get(merged_app(repo), "/api/users").await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("id").unwrap(), &json!(1));
    assert_eq!(items[0].get("name").unwrap(), &json!("user-1"));
}

#[tokio::test]
async fn generated_list_returns_all_ten_items() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 10).await;
    let (status, body) = json_get(merged_app(repo), "/api/users").await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 10);
    assert_eq!(items[9].get("id").unwrap(), &json!(10));
    assert_eq!(items[9].get("name").unwrap(), &json!("user-10"));
}

#[tokio::test]
async fn generated_get_by_id_returns_the_row() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 3).await;
    let (status, body) = json_get(merged_app(repo), "/api/users/2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("id").unwrap(), &json!(2));
    assert_eq!(body.get("name").unwrap(), &json!("user-2"));
}

#[tokio::test]
async fn generated_get_by_id_returns_404_when_missing() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 3).await;
    let (status, body) = json_get(merged_app(repo), "/api/users/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let errors = body.get("errors").and_then(|v| v.as_array()).unwrap();
    assert_eq!(errors.len(), 1);
    let first = &errors[0];
    assert_eq!(first.get("code").unwrap(), &json!("NOT_FOUND"));
    let msg = first.get("message").unwrap().as_str().unwrap();
    assert!(
        msg.contains("User"),
        "message should include entity name: {}",
        msg
    );
    assert!(msg.contains("999"), "message should include id: {}", msg);
}

#[tokio::test]
async fn generated_get_by_id_returns_400_when_id_not_positive_int() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    for path in ["/api/users/0", "/api/users/-1", "/api/users/abc"] {
        let (status, body) = json_get(merged_app(repo.clone()), path).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "path {} should be 400",
            path
        );
        let errors = body.get("errors").and_then(|v| v.as_array()).unwrap();
        assert_eq!(errors.len(), 1, "path {} should have one error entry", path);
        assert_eq!(errors[0].get("code").unwrap(), &json!("VALIDATION_ERROR"));
    }
}

#[tokio::test]
async fn custom_routes_still_respond_when_merged_with_generated() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    let (status, body) = json_get(merged_app(repo), "/api/custom-thing").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "custom": "yes" }));
}

#[tokio::test]
async fn generated_and_custom_coexist_under_the_same_server() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 2).await;
    let app = merged_app(repo);

    let (list_status, list_body) = json_get(app.clone(), "/api/users").await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(list_body.get("items").unwrap().as_array().unwrap().len(), 2);

    let (custom_status, custom_body) = json_get(app.clone(), "/api/custom-thing").await;
    assert_eq!(custom_status, StatusCode::OK);
    assert_eq!(custom_body, json!({ "custom": "yes" }));

    let (id_status, id_body) = json_get(app, "/api/users/1").await;
    assert_eq!(id_status, StatusCode::OK);
    assert_eq!(id_body.get("name").unwrap(), &json!("user-1"));
}

fn enriched_app(
    repo: Arc<InMemoryCrudRepository>,
    enrich_items: Option<deterministic::routes::EnrichItemsFn>,
    enrich_item: Option<deterministic::routes::EnrichItemFn>,
) -> Router {
    create_read_only_router(ReadOnlyRouterConfig {
        service: repo,
        entity_name: "User".to_string(),
        base_path: "/api/users".to_string(),
        id_type: IdType::Integer,
        enrich_items,
        enrich_item,
    })
}

fn tag_items_source() -> deterministic::routes::EnrichItemsFn {
    Arc::new(|rows: Vec<RowMap>| {
        Box::pin(async move {
            let out: Vec<RowMap> = rows
                .into_iter()
                .map(|mut r| {
                    r.insert(
                        "source".to_string(),
                        Value::String("enriched-list".to_string()),
                    );
                    r
                })
                .collect();
            Ok(out)
        })
    })
}

fn tag_item_source() -> deterministic::routes::EnrichItemFn {
    Arc::new(|mut row: RowMap| {
        Box::pin(async move {
            row.insert(
                "source".to_string(),
                Value::String("enriched-item".to_string()),
            );
            Ok(row)
        })
    })
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_items_transforms_every_row_on_list() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 3).await;
    let app = enriched_app(repo, Some(tag_items_source()), None);
    let (status, body) = json_get(app, "/api/users").await;
    assert_eq!(status, StatusCode::OK);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 3);
    for item in items {
        assert_eq!(item.get("source").unwrap(), &json!("enriched-list"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_items_can_change_row_count() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 5).await;
    let hook: deterministic::routes::EnrichItemsFn = Arc::new(|rows: Vec<RowMap>| {
        Box::pin(async move { Ok(rows.into_iter().take(2).collect()) })
    });
    let app = enriched_app(repo, Some(hook), None);
    let (status, body) = json_get(app, "/api/users").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("items").unwrap().as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_item_transforms_row_on_get_by_id() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 1).await;
    let app = enriched_app(repo, None, Some(tag_item_source()));
    let (status, body) = json_get(app, "/api/users/1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("source").unwrap(), &json!("enriched-item"));
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_item_does_not_run_when_id_is_not_found() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    let app = enriched_app(repo, None, Some(tag_item_source()));
    let (status, body) = json_get(app, "/api/users/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let errors = body.get("errors").and_then(|v| v.as_array()).unwrap();
    assert_eq!(errors[0].get("code").unwrap(), &json!("NOT_FOUND"));
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_items_error_returns_500_with_errors_envelope() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 1).await;
    let failing: deterministic::routes::EnrichItemsFn = Arc::new(|_rows| {
        Box::pin(async move {
            Err(deterministic::routes::RouteHookError::Repository(
                deterministic::error::RepositoryError::UnsupportedQuery(
                    "list enrichment blew up".to_string(),
                ),
            ))
        })
    });
    let app = enriched_app(repo, Some(failing), None);
    let (status, body) = json_get(app, "/api/users").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let errors = body.get("errors").and_then(|v| v.as_array()).unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].get("code").unwrap(), &json!("INTERNAL_ERROR"));
    let msg = errors[0].get("message").unwrap().as_str().unwrap();
    assert!(msg.contains("list enrichment blew up"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_item_error_returns_500_with_errors_envelope() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 1).await;
    let failing: deterministic::routes::EnrichItemFn = Arc::new(|_row| {
        Box::pin(async move {
            Err(deterministic::routes::RouteHookError::Repository(
                deterministic::error::RepositoryError::UnsupportedQuery(
                    "item enrichment blew up".to_string(),
                ),
            ))
        })
    });
    let app = enriched_app(repo, None, Some(failing));
    let (status, body) = json_get(app, "/api/users/1").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let errors = body.get("errors").and_then(|v| v.as_array()).unwrap();
    assert_eq!(errors[0].get("code").unwrap(), &json!("INTERNAL_ERROR"));
    let msg = errors[0].get("message").unwrap().as_str().unwrap();
    assert!(msg.contains("item enrichment blew up"), "message: {}", msg);
}

#[tokio::test(flavor = "current_thread")]
async fn enrich_items_and_enrich_item_are_independent_when_both_set() {
    let repo = Arc::new(InMemoryCrudRepository::new());
    seed(&repo, 1).await;
    let app = enriched_app(repo, Some(tag_items_source()), Some(tag_item_source()));
    let (list_status, list_body) = json_get(app.clone(), "/api/users").await;
    assert_eq!(list_status, StatusCode::OK);
    let items = list_body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items[0].get("source").unwrap(), &json!("enriched-list"));
    let (id_status, id_body) = json_get(app, "/api/users/1").await;
    assert_eq!(id_status, StatusCode::OK);
    assert_eq!(id_body.get("source").unwrap(), &json!("enriched-item"));
}
