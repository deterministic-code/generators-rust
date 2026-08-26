use deterministic::repositories::inmemory::InMemoryCrudRepository;
use deterministic::{CrudRepository, Repository, RepositoryError, RowMap};
use serde_json::{json, Value};

fn row(name: &str) -> RowMap {
    let mut m = RowMap::new();
    m.insert("name".to_string(), Value::from(name));
    m
}

#[tokio::test]
async fn query_returns_unsupported_error() {
    let repo = InMemoryCrudRepository::new();
    let err = repo.query("SELECT 1", &[]).await.unwrap_err();
    assert!(matches!(err, RepositoryError::UnsupportedQuery(_)));
}

#[tokio::test]
async fn add_assigns_incremental_ids() {
    let repo = InMemoryCrudRepository::new();
    let a = repo.add(row("alice")).await.unwrap();
    let b = repo.add(row("bob")).await.unwrap();
    assert_eq!(a.get("id").unwrap().as_i64(), Some(1));
    assert_eq!(b.get("id").unwrap().as_i64(), Some(2));
}

#[tokio::test]
async fn find_returns_none_for_missing() {
    let repo = InMemoryCrudRepository::new();
    assert!(repo
        .find(&serde_json::Value::from(99))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn find_returns_inserted_row() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("alice")).await.unwrap();
    let r = repo
        .find(&serde_json::Value::from(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(r.get("name").unwrap().as_str(), Some("alice"));
}

#[tokio::test]
async fn find_all_returns_sorted_by_id() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("a")).await.unwrap();
    repo.add(row("b")).await.unwrap();
    repo.add(row("c")).await.unwrap();
    let rows = repo.find_all().await.unwrap();
    assert_eq!(rows.len(), 3);
    let ids: Vec<i64> = rows
        .iter()
        .map(|r| r.get("id").unwrap().as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[tokio::test]
async fn find_by_filters_on_column() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("alice")).await.unwrap();
    repo.add(row("bob")).await.unwrap();
    repo.add(row("alice")).await.unwrap();
    let rows = repo.find_by("name", &Value::from("alice")).await.unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn update_modifies_existing_row() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("alice")).await.unwrap();
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alicia"));
    let updated = repo
        .update(&serde_json::Value::from(1), data)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.get("name").unwrap().as_str(), Some("alicia"));
}

#[tokio::test]
async fn update_returns_none_for_missing_id() {
    let repo = InMemoryCrudRepository::new();
    let r = repo
        .update(&serde_json::Value::from(42), RowMap::new())
        .await
        .unwrap();
    assert!(r.is_none());
}

#[tokio::test]
async fn delete_removes_existing_row() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("a")).await.unwrap();
    assert!(repo.delete(&serde_json::Value::from(1)).await.unwrap());
    assert!(repo
        .find(&serde_json::Value::from(1))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn delete_returns_false_for_missing_id() {
    let repo = InMemoryCrudRepository::new();
    assert!(!repo.delete(&serde_json::Value::from(99)).await.unwrap());
}

#[tokio::test]
async fn find_in_returns_empty_vec_for_empty_values() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("alice")).await.unwrap();
    let rows = repo.find_in("id", &[]).await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn find_in_returns_only_matching_rows() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("a")).await.unwrap();
    repo.add(row("b")).await.unwrap();
    repo.add(row("c")).await.unwrap();
    let rows = repo
        .find_in("id", &[Value::from(1), Value::from(3), Value::from(99)])
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let ids: Vec<i64> = rows
        .iter()
        .map(|r| r.get("id").unwrap().as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![1, 3]);
}

#[tokio::test]
async fn update_by_modifies_all_matching_rows_and_returns_them() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("alice")).await.unwrap();
    repo.add(row("bob")).await.unwrap();
    repo.add(row("alice")).await.unwrap();
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alicia"));
    let updated = repo
        .update_by("name", &Value::from("alice"), data)
        .await
        .unwrap();
    assert_eq!(updated.len(), 2);
    for r in &updated {
        assert_eq!(r.get("name").unwrap().as_str(), Some("alicia"));
    }
    let remaining_alice = repo.find_by("name", &Value::from("alice")).await.unwrap();
    assert!(remaining_alice.is_empty());
}

#[tokio::test]
async fn update_by_returns_empty_when_no_rows_match() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("alice")).await.unwrap();
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("x"));
    let updated = repo
        .update_by("name", &Value::from("nomatch"), data)
        .await
        .unwrap();
    assert!(updated.is_empty());
}

#[tokio::test]
async fn delete_by_removes_all_matching_rows_and_returns_count() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("a")).await.unwrap();
    repo.add(row("b")).await.unwrap();
    repo.add(row("a")).await.unwrap();
    let removed = repo.delete_by("name", &Value::from("a")).await.unwrap();
    assert_eq!(removed, 2);
    let remaining = repo.find_all().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].get("name").unwrap().as_str(), Some("b"));
}

#[tokio::test]
async fn delete_by_returns_zero_when_no_rows_match() {
    let repo = InMemoryCrudRepository::new();
    repo.add(row("a")).await.unwrap();
    let removed = repo
        .delete_by("name", &Value::from("nomatch"))
        .await
        .unwrap();
    assert_eq!(removed, 0);
}

fn is_uuid_v4_like(v: &Value) -> bool {
    let s = v.as_str().unwrap_or("");
    s.len() == 36 && s.chars().filter(|c| *c == '-').count() == 4
}

fn is_iso_millis_like(v: &Value) -> bool {
    let s = v.as_str().unwrap_or("");
    s.len() >= 20 && s.contains('T') && (s.ends_with('Z') || s.contains('+'))
}

#[tokio::test]
async fn add_injects_uuid_created_updated_when_standard_columns_enabled() {
    let repo = InMemoryCrudRepository::new();
    let inserted = repo.add(row("alice")).await.unwrap();
    assert!(is_uuid_v4_like(
        inserted.get("uuid").expect("uuid should be injected")
    ));
    assert!(is_iso_millis_like(
        inserted.get("created").expect("created should be injected")
    ));
    assert!(is_iso_millis_like(
        inserted.get("updated").expect("updated should be injected")
    ));
    assert_eq!(
        inserted.get("created").unwrap(),
        inserted.get("updated").unwrap(),
        "created and updated should match on insert"
    );
}

#[tokio::test]
async fn add_caller_supplied_uuid_and_timestamps_override_defaults() {
    let repo = InMemoryCrudRepository::new();
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("caller-driven"));
    data.insert(
        "uuid".to_string(),
        Value::from("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
    );
    data.insert(
        "created".to_string(),
        Value::from("2000-01-01T00:00:00.000Z"),
    );
    data.insert(
        "updated".to_string(),
        Value::from("2000-01-02T00:00:00.000Z"),
    );
    let inserted = repo.add(data).await.unwrap();
    assert_eq!(
        inserted.get("uuid").unwrap().as_str(),
        Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
    );
    assert_eq!(
        inserted.get("created").unwrap().as_str(),
        Some("2000-01-01T00:00:00.000Z")
    );
    assert_eq!(
        inserted.get("updated").unwrap().as_str(),
        Some("2000-01-02T00:00:00.000Z")
    );
}

#[tokio::test]
async fn add_skips_standard_columns_when_flag_disabled() {
    let repo = InMemoryCrudRepository::with_standard_columns(false);
    let inserted = repo.add(row("alice")).await.unwrap();
    assert!(
        inserted.get("uuid").is_none(),
        "uuid must not be injected when flag off"
    );
    assert!(
        inserted.get("created").is_none(),
        "created must not be injected when flag off"
    );
    assert!(
        inserted.get("updated").is_none(),
        "updated must not be injected when flag off"
    );
    assert_eq!(inserted.get("id").unwrap().as_i64(), Some(1));
    assert_eq!(inserted.get("name").unwrap().as_str(), Some("alice"));
}

#[tokio::test]
async fn update_bumps_updated_and_overrides_caller_supplied_updated() {
    let repo = InMemoryCrudRepository::new();
    let inserted = repo.add(row("alice")).await.unwrap();
    let original_created = inserted.get("created").unwrap().clone();
    let original_updated = inserted
        .get("updated")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alicia"));
    data.insert(
        "updated".to_string(),
        Value::from("2000-01-01T00:00:00.000Z"),
    );
    let updated = repo.update(&Value::from(1), data).await.unwrap().unwrap();
    assert_eq!(updated.get("name").unwrap().as_str(), Some("alicia"));
    assert_eq!(
        updated.get("created").unwrap(),
        &original_created,
        "created must not change on update"
    );
    let updated_value = updated.get("updated").unwrap().as_str().unwrap();
    assert_ne!(
        updated_value, "2000-01-01T00:00:00.000Z",
        "auto-bump must override caller-supplied updated"
    );
    assert!(
        updated_value > original_updated.as_str(),
        "updated must advance; original {} new {}",
        original_updated,
        updated_value
    );
}

#[tokio::test]
async fn update_does_not_bump_updated_when_flag_disabled() {
    let repo = InMemoryCrudRepository::with_standard_columns(false);
    let mut initial = RowMap::new();
    initial.insert("name".to_string(), Value::from("alice"));
    initial.insert("updated".to_string(), Value::from("frozen-original"));
    repo.add(initial).await.unwrap();
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alicia"));
    let updated = repo.update(&Value::from(1), data).await.unwrap().unwrap();
    assert_eq!(
        updated.get("updated").unwrap().as_str(),
        Some("frozen-original"),
        "updated must not be auto-bumped when flag off"
    );
}

#[tokio::test]
async fn update_by_bumps_updated_on_every_matching_row() {
    let repo = InMemoryCrudRepository::new();
    let first = repo.add(row("alice")).await.unwrap();
    repo.add(row("bob")).await.unwrap();
    let third = repo.add(row("alice")).await.unwrap();
    let first_original_updated = first.get("updated").unwrap().as_str().unwrap().to_string();
    let third_original_updated = third.get("updated").unwrap().as_str().unwrap().to_string();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alicia"));
    let updated = repo
        .update_by("name", &Value::from("alice"), data)
        .await
        .unwrap();
    assert_eq!(updated.len(), 2);
    for r in &updated {
        let u = r.get("updated").unwrap().as_str().unwrap();
        assert_ne!(u, first_original_updated.as_str());
        assert_ne!(u, third_original_updated.as_str());
    }
}

#[tokio::test]
async fn update_by_does_not_bump_updated_when_flag_disabled() {
    let repo = InMemoryCrudRepository::with_standard_columns(false);
    let mut initial = RowMap::new();
    initial.insert("name".to_string(), Value::from("alice"));
    initial.insert("updated".to_string(), Value::from("frozen-original"));
    repo.add(initial).await.unwrap();
    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alicia"));
    let updated = repo
        .update_by("name", &Value::from("alice"), data)
        .await
        .unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(
        updated[0].get("updated").unwrap().as_str(),
        Some("frozen-original")
    );
}

#[tokio::test]
async fn add_assigns_incremental_ids_with_flag_disabled() {
    let repo = InMemoryCrudRepository::with_standard_columns(false);
    let a = repo.add(row("alice")).await.unwrap();
    let b = repo.add(row("bob")).await.unwrap();
    assert_eq!(a.get("id").unwrap().as_i64(), Some(1));
    assert_eq!(b.get("id").unwrap().as_i64(), Some(2));
}

#[tokio::test]
async fn composite_identity_find_update_delete() {
    let repo = InMemoryCrudRepository::with_standard_columns(false)
        .with_primary_keys(["left_id", "right_id"]);
    let mut data = RowMap::new();
    data.insert("left_id".to_string(), Value::from(1i64));
    data.insert("right_id".to_string(), Value::from(2i64));
    data.insert("label".to_string(), Value::from("ab"));
    repo.add(data).await.unwrap();
    let id = json!({ "left_id": 1, "right_id": 2 });
    let found = repo.find(&id).await.unwrap().unwrap();
    assert_eq!(found.get("label").unwrap().as_str(), Some("ab"));
    let mut patch = RowMap::new();
    patch.insert("label".to_string(), Value::from("changed"));
    let updated = repo.update(&id, patch).await.unwrap().unwrap();
    assert_eq!(updated.get("label").unwrap().as_str(), Some("changed"));
    assert!(repo.delete(&id).await.unwrap());
    assert!(repo.find(&id).await.unwrap().is_none());
}
