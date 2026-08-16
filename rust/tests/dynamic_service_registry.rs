use std::collections::HashMap;
use std::sync::Arc;

use deterministic::repositories::inmemory::InMemoryCrudRepository;
use deterministic::{DynamicService, GenericCrudService};
use serde_json::{json, Value};

type Registry = HashMap<String, Arc<dyn DynamicService>>;

fn build_registry(entity_names: &[&str]) -> Registry {
    let mut reg: Registry = HashMap::new();
    for name in entity_names {
        let service: Arc<dyn DynamicService> = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        reg.insert((*name).to_string(), service);
    }
    reg
}

async fn invoke(
    reg: &Registry,
    entity: &str,
    method: &str,
    args: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let svc = reg
        .get(entity)
        .ok_or_else(|| format!("unknown service: {}", entity))?;
    Ok(svc.invoke(method, args).await?)
}

#[tokio::test]
async fn registry_dispatches_create_to_correct_service() {
    let reg = build_registry(&["notification", "notification_type"]);
    let nt = invoke(
        &reg,
        "notification_type",
        "create",
        json!({ "name": "sample" }),
    )
    .await
    .unwrap();
    assert_eq!(nt["name"], json!("sample"));

    let n = invoke(&reg, "notification", "create", json!({ "text": "hi" }))
        .await
        .unwrap();
    assert_eq!(n["text"], json!("hi"));

    let all_nt = invoke(&reg, "notification_type", "findAll", json!({}))
        .await
        .unwrap();
    assert_eq!(all_nt.as_array().unwrap().len(), 1);
    let all_n = invoke(&reg, "notification", "findAll", json!({}))
        .await
        .unwrap();
    assert_eq!(all_n.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn full_crud_lifecycle_through_dynamic_service() {
    let reg = build_registry(&["widget"]);

    let created = invoke(&reg, "widget", "create", json!({ "label": "alpha" }))
        .await
        .unwrap();
    let id = created["id"].clone();

    let found = invoke(&reg, "widget", "findById", json!({ "id": id.clone() }))
        .await
        .unwrap();
    assert_eq!(found["label"], json!("alpha"));

    let updated = invoke(
        &reg,
        "widget",
        "update",
        json!({ "id": id.clone(), "data": { "label": "beta" } }),
    )
    .await
    .unwrap();
    assert_eq!(updated["label"], json!("beta"));

    let deleted = invoke(&reg, "widget", "delete", json!({ "id": id.clone() }))
        .await
        .unwrap();
    assert_eq!(deleted["deleted"], json!(true));

    let after = invoke(&reg, "widget", "findAll", json!({})).await.unwrap();
    assert!(after.as_array().unwrap().is_empty());
}
