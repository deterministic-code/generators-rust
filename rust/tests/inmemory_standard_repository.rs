use deterministic::repositories::inmemory::InMemoryStandardRepository;
use deterministic::{CrudRepository, RowMap};
use serde_json::Value;

fn row(name: &str) -> RowMap {
    let mut m = RowMap::new();
    m.insert("name".to_string(), Value::from(name));
    m
}

#[tokio::test]
async fn add_populates_standard_fields() {
    let repo = InMemoryStandardRepository::new();
    let r = repo.add(row("alice")).await.unwrap();
    assert_eq!(r.get("id").unwrap().as_i64(), Some(1));
    assert!(r.get("uuid").unwrap().as_str().is_some());
    assert!(r.get("created").unwrap().as_str().is_some());
    assert!(r.get("updated").unwrap().as_str().is_some());
    assert_eq!(
        r.get("created").unwrap().as_str(),
        r.get("updated").unwrap().as_str()
    );
}

#[tokio::test]
async fn update_refreshes_updated_field() {
    let repo = InMemoryStandardRepository::new();
    let initial = repo.add(row("alice")).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alicia"));
    let updated = repo
        .update(&serde_json::Value::from(1), data)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        initial.get("updated").unwrap().as_str(),
        updated.get("updated").unwrap().as_str()
    );
    assert_eq!(
        initial.get("created").unwrap().as_str(),
        updated.get("created").unwrap().as_str()
    );
}

#[tokio::test]
async fn delete_removes_row() {
    let repo = InMemoryStandardRepository::new();
    repo.add(row("a")).await.unwrap();
    assert!(repo.delete(&serde_json::Value::from(1)).await.unwrap());
}
