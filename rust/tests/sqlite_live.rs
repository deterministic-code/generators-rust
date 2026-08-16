use deterministic::mappings::{
    EntityFieldMap, FieldMappingTranslator, SqliteBooleanConverter, TypeFieldConverter,
};
use deterministic::repositories::sqlite::{
    SqliteColumnDecoder, SqliteCrudRepository, SqliteDatasource, SqliteDatasourceOptions,
    SqliteRepository, SqliteStandardRepository,
};
use deterministic::{CrudRepository, Datasource, IdType, Repository, RepositoryError, RowMap};
use serde_json::Value;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;
use std::collections::BTreeMap;
use std::sync::Arc;

async fn open_in_memory() -> Arc<SqliteDatasource> {
    let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
    ds.open().await.expect("open in-memory sqlite");
    Arc::new(ds)
}

async fn create_contacts_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE contacts (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create contacts");
}

async fn create_items_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT NOT NULL, name TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create items");
}

#[tokio::test]
async fn sqlite_repository_executes_raw_sql() {
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteRepository::new(Arc::clone(&ds));
    let rows = repo.query("SELECT 1 AS one", &[]).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("one").unwrap().as_i64(), Some(1));
}

#[tokio::test]
async fn sqlite_crud_repository_full_lifecycle() {
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    let added = repo.add(data).await.unwrap();
    assert_eq!(added.get("id").unwrap().as_i64(), Some(1));
    assert_eq!(added.get("name").unwrap().as_str(), Some("alice"));

    let found = repo
        .find(&serde_json::Value::from(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.get("name").unwrap().as_str(), Some("alice"));

    let mut update = RowMap::new();
    update.insert("name".to_string(), Value::from("alicia"));
    let updated = repo
        .update(&serde_json::Value::from(1), update)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.get("name").unwrap().as_str(), Some("alicia"));

    let by = repo.find_by("name", &Value::from("alicia")).await.unwrap();
    assert_eq!(by.len(), 1);

    assert!(repo.delete(&serde_json::Value::from(1)).await.unwrap());
    assert!(repo
        .find(&serde_json::Value::from(1))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_standard_repository_populates_uuid_and_timestamps() {
    let ds = open_in_memory().await;
    create_items_table(&ds).await;
    let repo = SqliteStandardRepository::new(Arc::clone(&ds), "items").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("widget"));
    let row = repo.add(data).await.unwrap();
    assert!(row.get("uuid").unwrap().as_str().unwrap().len() >= 32);
    assert!(row.get("created").unwrap().as_str().is_some());
    assert!(row.get("updated").unwrap().as_str().is_some());
}

#[tokio::test]
async fn sqlite_crud_repository_add_translates_custom_pk_through_field_mapping() {
    // why: regression for legacy_contacts — physical-name lookup of the renamed PK used to return None, so add() 500'd; the row must round-trip on the logical key.
    let ds = open_in_memory().await;
    ds.query(
        "CREATE TABLE OldContactsTbl (CntID TEXT PRIMARY KEY, FirstNm TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create OldContactsTbl");

    let mut entity_map = EntityFieldMap::new();
    entity_map.insert("key".to_string(), "CntID".to_string());
    entity_map.insert("first_name".to_string(), "FirstNm".to_string());
    let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).unwrap());

    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "OldContactsTbl")
        .unwrap()
        .with_primary_key("key")
        .unwrap()
        .with_field_mapping_translator(translator);

    let mut data = RowMap::new();
    data.insert("key".to_string(), Value::from("sample-0"));
    data.insert("first_name".to_string(), Value::from("Alice"));
    let added = repo.add(data).await.expect("add legacy_contact row");
    assert_eq!(added.get("key").unwrap().as_str(), Some("sample-0"));
    assert_eq!(added.get("first_name").unwrap().as_str(), Some("Alice"));
    assert!(added.get("CntID").is_none(), "physical name must not leak");
}

#[tokio::test]
async fn sqlite_crud_rejects_invalid_table_name() {
    let ds = open_in_memory().await;
    let result = SqliteCrudRepository::new(Arc::clone(&ds), "Contacts; DROP TABLE x;--");
    let Err(err) = result else {
        panic!("expected InvalidIdentifier");
    };
    assert!(matches!(
        err,
        deterministic::RepositoryError::InvalidIdentifier(_)
    ));
}

#[tokio::test]
async fn sqlite_crud_add_fills_uuid_when_table_has_uuid_column_and_body_omits_it() {
    // why: SQLite has no UUID() builtin, so renamed uuid columns (e.g. notifications.guid) ship NOT NULL with no DEFAULT; the repo must fill the uuid logical key client-side or every POST 500s with "NOT NULL constraint failed: notifications.guid".
    let ds = open_in_memory().await;
    ds.query(
        "CREATE TABLE notifications (key TEXT PRIMARY KEY, guid TEXT NOT NULL UNIQUE, text TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create notifications");

    let mut entity_map = EntityFieldMap::new();
    entity_map.insert("uuid".to_string(), "guid".to_string());
    let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).unwrap());

    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "notifications")
        .unwrap()
        .with_primary_key("key")
        .unwrap()
        .with_field_mapping_translator(translator);

    let mut data = RowMap::new();
    data.insert("key".to_string(), Value::from("sample-0"));
    data.insert("text".to_string(), Value::from("hello"));
    let added = repo.add(data).await.expect("add notifications row");
    let returned_uuid = added
        .get("uuid")
        .and_then(|v| v.as_str())
        .expect("uuid logical key returned");
    assert_eq!(returned_uuid.len(), 36, "uuid must be a v4 string");
    assert!(added.get("guid").is_none(), "physical name must not leak");
}

#[tokio::test]
async fn sqlite_crud_add_preserves_caller_supplied_uuid() {
    // why: client-side fill must not clobber an explicit uuid from the request body.
    let ds = open_in_memory().await;
    ds.query(
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT NOT NULL UNIQUE, name TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create widgets");
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "widgets").unwrap();

    let mut data = RowMap::new();
    data.insert(
        "uuid".to_string(),
        Value::from("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
    );
    data.insert("name".to_string(), Value::from("explicit"));
    let added = repo.add(data).await.expect("add widget");
    assert_eq!(
        added.get("uuid").and_then(|v| v.as_str()),
        Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
    );
}

#[tokio::test]
async fn sqlite_crud_add_skips_uuid_fill_when_table_has_no_uuid_column() {
    // why: tables without a uuid column (legacy_contact-style) must not have an extra column injected.
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("plain"));
    let added = repo.add(data).await.expect("add contact");
    assert!(
        added.get("uuid").is_none(),
        "no uuid expected when table lacks the column"
    );
    assert_eq!(added.get("name").unwrap().as_str(), Some("plain"));
}

#[tokio::test]
async fn sqlite_crud_boolean_converter_promotes_integer_storage_to_bool_on_read() {
    // why: e2e_rust_mysql regression — the converter pipeline must promote integer-stored bools to Value::Bool on read; sqlite path mirrors mysql's TINYINT(1) shape.
    let ds = open_in_memory().await;
    ds.query(
        "CREATE TABLE flags (id INTEGER PRIMARY KEY AUTOINCREMENT, is_active INTEGER NOT NULL, is_system INTEGER NOT NULL, label TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create flags");

    let mut col_types = BTreeMap::new();
    col_types.insert("is_active".to_string(), "boolean".to_string());
    col_types.insert("is_system".to_string(), "boolean".to_string());

    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "flags")
        .unwrap()
        .with_column_datasource_types(col_types)
        .with_converters(vec![
            Arc::new(SqliteBooleanConverter) as Arc<dyn TypeFieldConverter>
        ]);

    let mut data = RowMap::new();
    data.insert("is_active".to_string(), Value::Bool(true));
    data.insert("is_system".to_string(), Value::Bool(false));
    data.insert("label".to_string(), Value::from("hello"));
    let added = repo.add(data).await.expect("add flags row");

    // The returned row from add() — the surface that an HTTP POST handler renders — must be Value::Bool, not Value::Number.
    assert_eq!(
        added.get("is_active"),
        Some(&Value::Bool(true)),
        "is_active must serialize as Bool(true), not Number(1)"
    );
    assert_eq!(
        added.get("is_system"),
        Some(&Value::Bool(false)),
        "is_system must serialize as Bool(false), not Number(0)"
    );

    let id = added.get("id").and_then(|v| v.as_i64()).expect("id");
    let found = repo
        .find(&Value::from(id))
        .await
        .expect("find")
        .expect("row exists");
    assert_eq!(found.get("is_active"), Some(&Value::Bool(true)));
    assert_eq!(found.get("is_system"), Some(&Value::Bool(false)));

    let by = repo
        .find_by("is_active", &Value::Bool(true))
        .await
        .expect("find_by");
    assert_eq!(
        by.len(),
        1,
        "find_by should bind Bool(true) → integer 1 in WHERE"
    );
    assert_eq!(by[0].get("is_active"), Some(&Value::Bool(true)));
}

#[tokio::test]
async fn custom_column_decoder_overrides_default_for_matching_type_name() {
    // why: locks the architecture contract — a user-registered SqliteColumnDecoder must be consulted before the default fallback, and a non-None return wins. This is the open extension point row_convert.rs used to block.
    struct ShoutingTextDecoder;
    impl SqliteColumnDecoder for ShoutingTextDecoder {
        fn decode(
            &self,
            row: &SqliteRow,
            idx: usize,
            type_name: &str,
        ) -> Result<Option<Value>, RepositoryError> {
            if type_name == "TEXT" {
                let s: String = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                return Ok(Some(Value::String(s.to_uppercase())));
            }
            Ok(None)
        }
    }

    let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory())
        .with_decoder(Arc::new(ShoutingTextDecoder));
    ds.open().await.expect("open sqlite");
    let ds = Arc::new(ds);
    ds.query(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL, count INTEGER NOT NULL)",
        &[],
    )
    .await
    .expect("create notes");
    ds.query(
        "INSERT INTO notes (body, count) VALUES (?, ?)",
        &[Value::from("quiet"), Value::from(7)],
    )
    .await
    .expect("seed row");

    let rows = ds
        .query("SELECT body, count FROM notes", &[])
        .await
        .expect("select");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("body").and_then(|v| v.as_str()),
        Some("QUIET"),
        "custom decoder must have shouted the TEXT column"
    );
    assert_eq!(
        rows[0].get("count").and_then(|v| v.as_i64()),
        Some(7),
        "INTEGER column must fall through to the default decoder"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sqlite_crud_concurrent_add_returns_distinct_ids() {
    // why: regression — concurrent POSTs used to return the same id because INSERT + SELECT last_insert_rowid() ran as separate pooled-connection queries.
    let mut ds = SqliteDatasource::new(SqliteDatasourceOptions {
        url: "sqlite::memory:?cache=shared".to_string(),
        max_connections: 4,
    });
    ds.open().await.expect("open shared in-memory sqlite");
    let ds = Arc::new(ds);
    ds.query(
        "CREATE TABLE email_accounts (id INTEGER PRIMARY KEY AUTOINCREMENT, address TEXT NOT NULL UNIQUE)",
        &[],
    )
    .await
    .expect("create email_accounts");
    let repo = Arc::new(SqliteCrudRepository::new(Arc::clone(&ds), "email_accounts").unwrap());

    let workers = 8usize;
    let mut handles = Vec::with_capacity(workers);
    for w in 0..workers {
        let repo = Arc::clone(&repo);
        handles.push(tokio::spawn(async move {
            let mut data = RowMap::new();
            data.insert(
                "address".to_string(),
                Value::from(format!("user-{w}@example.com")),
            );
            (w, repo.add(data).await.expect("concurrent add"))
        }));
    }
    let mut returned = Vec::with_capacity(workers);
    for h in handles {
        returned.push(h.await.expect("join task"));
    }

    let mut ids: Vec<i64> = returned
        .iter()
        .map(|(_, row)| row.get("id").and_then(|v| v.as_i64()).expect("id"))
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        workers,
        "every concurrent add must return a distinct id"
    );

    for (w, row) in &returned {
        let id = row.get("id").and_then(|v| v.as_i64()).expect("id");
        let address = row
            .get("address")
            .and_then(|v| v.as_str())
            .expect("address");
        assert_eq!(address, format!("user-{w}@example.com"));
        let found = repo
            .find(&Value::from(id))
            .await
            .expect("find")
            .expect("row exists");
        assert_eq!(
            found.get("address").and_then(|v| v.as_str()),
            Some(address),
            "returned id must point to the row this worker actually inserted"
        );
    }
}

#[tokio::test]
async fn sqlite_find_in_returns_only_matching_rows() {
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();

    for n in ["a", "b", "c", "d"] {
        let mut data = RowMap::new();
        data.insert("name".to_string(), Value::from(n));
        repo.add(data).await.unwrap();
    }

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
async fn sqlite_find_in_returns_empty_vec_for_empty_values() {
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    repo.add(data).await.unwrap();

    let rows = repo.find_in("id", &[]).await.unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn sqlite_update_by_updates_all_matching_rows_and_returns_them() {
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();

    for n in ["alice", "bob", "alice"] {
        let mut data = RowMap::new();
        data.insert("name".to_string(), Value::from(n));
        repo.add(data).await.unwrap();
    }

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
    let remaining = repo.find_by("name", &Value::from("alice")).await.unwrap();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn sqlite_update_by_returns_empty_when_no_rows_match() {
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("x"));
    let updated = repo
        .update_by("name", &Value::from("nomatch"), data)
        .await
        .unwrap();
    assert!(updated.is_empty());
}

#[tokio::test]
async fn sqlite_delete_by_removes_all_matching_rows_and_returns_count() {
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();

    for n in ["a", "b", "a"] {
        let mut data = RowMap::new();
        data.insert("name".to_string(), Value::from(n));
        repo.add(data).await.unwrap();
    }

    let removed = repo.delete_by("name", &Value::from("a")).await.unwrap();
    assert_eq!(removed, 2);
    let remaining = repo.find_all().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].get("name").unwrap().as_str(), Some("b"));
}

async fn create_versioned_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE versioned (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, updated TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create versioned");
}

#[tokio::test]
async fn update_with_expected_matches_current_updated_and_applies_update() {
    let ds = open_in_memory().await;
    create_versioned_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "versioned").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    data.insert(
        "updated".to_string(),
        Value::from("2026-01-01T00:00:00.000Z"),
    );
    let added = repo.add(data).await.unwrap();
    let id = added.get("id").unwrap().clone();

    let mut patch = RowMap::new();
    patch.insert("name".to_string(), Value::from("alicia"));
    patch.insert(
        "updated".to_string(),
        Value::from("2026-01-02T00:00:00.000Z"),
    );
    let result = repo
        .update_with_expected(&id, patch, Some("2026-01-01T00:00:00.000Z"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.get("name").unwrap().as_str(), Some("alicia"));
}

#[tokio::test]
async fn update_with_expected_returns_concurrency_conflict_on_stale_stamp() {
    let ds = open_in_memory().await;
    create_versioned_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "versioned").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    data.insert(
        "updated".to_string(),
        Value::from("2026-01-01T00:00:00.000Z"),
    );
    let added = repo.add(data).await.unwrap();
    let id = added.get("id").unwrap().clone();

    let mut patch = RowMap::new();
    patch.insert("name".to_string(), Value::from("alicia"));
    let err = repo
        .update_with_expected(&id, patch, Some("stale-timestamp"))
        .await
        .unwrap_err();
    assert!(matches!(err, RepositoryError::ConcurrencyConflict(_)));
}

#[tokio::test]
async fn update_with_expected_none_matches_plain_update_behavior() {
    let ds = open_in_memory().await;
    create_versioned_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "versioned").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    data.insert("updated".to_string(), Value::from("stamp"));
    let added = repo.add(data).await.unwrap();
    let id = added.get("id").unwrap().clone();

    let mut patch = RowMap::new();
    patch.insert("name".to_string(), Value::from("alicia"));
    let result = repo
        .update_with_expected(&id, patch, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.get("name").unwrap().as_str(), Some("alicia"));
}

#[tokio::test]
async fn delete_with_expected_returns_concurrency_conflict_on_stale_stamp() {
    let ds = open_in_memory().await;
    create_versioned_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "versioned").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    data.insert("updated".to_string(), Value::from("stamp"));
    let added = repo.add(data).await.unwrap();
    let id = added.get("id").unwrap().clone();

    let err = repo
        .delete_with_expected(&id, Some("stale-timestamp"))
        .await
        .unwrap_err();
    assert!(matches!(err, RepositoryError::ConcurrencyConflict(_)));
    let remaining = repo.find(&id).await.unwrap();
    assert!(
        remaining.is_some(),
        "delete must not have run when OCC fails"
    );
}

#[tokio::test]
async fn delete_with_expected_matches_current_stamp_and_deletes() {
    let ds = open_in_memory().await;
    create_versioned_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "versioned").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    data.insert("updated".to_string(), Value::from("stamp"));
    let added = repo.add(data).await.unwrap();
    let id = added.get("id").unwrap().clone();

    let removed = repo.delete_with_expected(&id, Some("stamp")).await.unwrap();
    assert!(removed);
    let remaining = repo.find(&id).await.unwrap();
    assert!(remaining.is_none());
}

#[tokio::test]
async fn sqlite_delete_by_returns_zero_when_no_rows_match() {
    let ds = open_in_memory().await;
    create_contacts_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("a"));
    repo.add(data).await.unwrap();

    let removed = repo
        .delete_by("name", &Value::from("nomatch"))
        .await
        .unwrap();
    assert_eq!(removed, 0);
}

async fn create_members_uuid_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE members (id TEXT PRIMARY KEY, handle TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create members");
}

#[tokio::test]
async fn sqlite_crud_generates_uuid_primary_key_when_id_type_uuid() {
    let ds = open_in_memory().await;
    create_members_uuid_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "members")
        .unwrap()
        .with_id_type(IdType::Uuid);

    let mut data = RowMap::new();
    data.insert("handle".to_string(), Value::from("ada"));
    let added = repo.add(data).await.unwrap();

    let id = added.get("id").expect("id present");
    let id_str = id.as_str().expect("id is a string uuid, not null");
    assert_eq!(id_str.len(), 36, "canonical hyphenated uuid: {id_str}");
    uuid::Uuid::parse_str(id_str).expect("id is a valid uuid");

    let found = repo.find(id).await.unwrap().expect("round-trips by uuid");
    assert_eq!(found.get("handle").unwrap().as_str(), Some("ada"));
}

#[tokio::test]
async fn sqlite_crud_preserves_caller_supplied_uuid_primary_key() {
    let ds = open_in_memory().await;
    create_members_uuid_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "members")
        .unwrap()
        .with_id_type(IdType::Uuid);

    let supplied = "550e8400-e29b-41d4-a716-446655440000";
    let mut data = RowMap::new();
    data.insert("id".to_string(), Value::from(supplied));
    data.insert("handle".to_string(), Value::from("grace"));
    let added = repo.add(data).await.unwrap();

    assert_eq!(added.get("id").unwrap().as_str(), Some(supplied));
}
