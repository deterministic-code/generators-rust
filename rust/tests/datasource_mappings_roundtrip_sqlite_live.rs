use std::collections::BTreeMap;
use std::sync::Arc;

use deterministic::mappings::{
    get_entity_field_map, parse_field_mappings, EntityFieldMap, FieldMappingTranslator,
    TypeFieldConverter,
};
use deterministic::repositories::datasource_middleware::DatabaseBackend;
use deterministic::repositories::sqlite::{
    SqliteCrudRepository, SqliteDatasource, SqliteDatasourceOptions, SqliteStandardRepository,
};
use deterministic::{CrudRepository, Datasource, RowMap};
use serde_json::{json, Value};

// Mirror of typescript/src/repositories/__tests__/datasource-mappings-roundtrip.integration.test.ts (PR #974) — 5 scenarios YAML->parse_field_mappings->FieldMappingTranslator->live sqlite CRUD with physical-column DDL.

fn user_spec() -> Value {
    json!({
        "datasource_mappings": [
            {
                "user": {
                    "source": "app_user",
                    "field_mappings": [
                        { "id": { "source": "USER_ID" } },
                        { "email": { "source": "EMAIL_ADDRESS" } },
                        { "name": { "source": "FULL_NAME" } },
                        { "createdAt": { "source": "RECORD_CREATED_AT" } },
                    ]
                }
            }
        ]
    })
}

fn user_entity_map() -> EntityFieldMap {
    let mappings = parse_field_mappings(&user_spec()).expect("parse_field_mappings");
    get_entity_field_map(&mappings, "user")
}

async fn open_in_memory() -> Arc<SqliteDatasource> {
    let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
    ds.open().await.expect("open in-memory sqlite");
    Arc::new(ds)
}

async fn create_backend_app_user_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE app_user (\
            USER_ID INTEGER PRIMARY KEY AUTOINCREMENT, \
            EMAIL_ADDRESS TEXT NOT NULL, \
            FULL_NAME TEXT NOT NULL, \
            RECORD_CREATED_AT TEXT NOT NULL\
        )",
        &[],
    )
    .await
    .expect("create app_user");
}

#[tokio::test]
async fn sqlite_field_mappings_parse_and_translator_wires_logical_to_physical() {
    let entity_map = user_entity_map();
    let translator = FieldMappingTranslator::new(Some(&entity_map)).expect("translator");

    assert_eq!(translator.to_physical("email"), "EMAIL_ADDRESS");
    assert_eq!(translator.to_physical("id"), "USER_ID");
    assert_eq!(translator.to_physical("name"), "FULL_NAME");
    assert_eq!(translator.to_physical("createdAt"), "RECORD_CREATED_AT");

    assert_eq!(translator.to_logical("FULL_NAME"), "name");
    assert_eq!(translator.to_logical("EMAIL_ADDRESS"), "email");

    assert_eq!(translator.to_physical("unknown"), "unknown");
    assert_eq!(translator.to_logical("UNKNOWN_COL"), "UNKNOWN_COL");
}

#[tokio::test]
async fn sqlite_crud_insert_via_logical_names_populates_physical_columns() {
    let ds = open_in_memory().await;
    create_backend_app_user_table(&ds).await;

    let entity_map = user_entity_map();
    let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).expect("translator"));
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "app_user")
        .expect("repo")
        .with_primary_key("id")
        .expect("pk")
        .with_field_mapping_translator(translator);

    let mut row = RowMap::new();
    row.insert("email".to_string(), Value::from("alice@a.com"));
    row.insert("name".to_string(), Value::from("Alice"));
    row.insert(
        "createdAt".to_string(),
        Value::from("2026-01-01T00:00:00.000Z"),
    );
    let added = repo.add(row).await.expect("add row");

    assert_eq!(
        added.get("email").and_then(|v| v.as_str()),
        Some("alice@a.com")
    );
    assert_eq!(added.get("name").and_then(|v| v.as_str()), Some("Alice"));
    assert_eq!(
        added.get("createdAt").and_then(|v| v.as_str()),
        Some("2026-01-01T00:00:00.000Z"),
    );
    assert!(added.get("id").and_then(|v| v.as_i64()).is_some());
    let mut keys: Vec<&str> = added.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, vec!["createdAt", "email", "id", "name"]);
    assert!(
        added.get("EMAIL_ADDRESS").is_none(),
        "physical name must not leak"
    );

    let raw = ds
        .query(
            "SELECT USER_ID, EMAIL_ADDRESS, FULL_NAME, RECORD_CREATED_AT FROM app_user",
            &[],
        )
        .await
        .expect("raw select");
    assert_eq!(raw.len(), 1);
    assert_eq!(
        raw[0].get("USER_ID").and_then(|v| v.as_i64()),
        added.get("id").and_then(|v| v.as_i64()),
    );
    assert_eq!(
        raw[0].get("EMAIL_ADDRESS").and_then(|v| v.as_str()),
        Some("alice@a.com"),
    );
    assert_eq!(
        raw[0].get("FULL_NAME").and_then(|v| v.as_str()),
        Some("Alice")
    );
    assert_eq!(
        raw[0].get("RECORD_CREATED_AT").and_then(|v| v.as_str()),
        Some("2026-01-01T00:00:00.000Z"),
    );
}

#[tokio::test]
async fn sqlite_crud_find_by_logical_pk_returns_logical_named_row() {
    let ds = open_in_memory().await;
    create_backend_app_user_table(&ds).await;

    ds.query(
        "INSERT INTO app_user (EMAIL_ADDRESS, FULL_NAME, RECORD_CREATED_AT) VALUES (?, ?, ?)",
        &[
            Value::from("bob@b.com"),
            Value::from("Bob"),
            Value::from("2026-02-02T00:00:00.000Z"),
        ],
    )
    .await
    .expect("seed row");
    let seeded = ds
        .query("SELECT USER_ID FROM app_user", &[])
        .await
        .expect("seeded id");
    let seeded_id = seeded[0]
        .get("USER_ID")
        .and_then(|v| v.as_i64())
        .expect("USER_ID");

    let entity_map = user_entity_map();
    let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).expect("translator"));
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "app_user")
        .expect("repo")
        .with_primary_key("id")
        .expect("pk")
        .with_field_mapping_translator(translator);

    let row = repo
        .find(&Value::from(seeded_id))
        .await
        .expect("find")
        .expect("row exists");

    let mut keys: Vec<&str> = row.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, vec!["createdAt", "email", "id", "name"]);
    assert_eq!(row.get("id").and_then(|v| v.as_i64()), Some(seeded_id));
    assert_eq!(row.get("email").and_then(|v| v.as_str()), Some("bob@b.com"));
    assert_eq!(row.get("name").and_then(|v| v.as_str()), Some("Bob"));
    assert_eq!(
        row.get("createdAt").and_then(|v| v.as_str()),
        Some("2026-02-02T00:00:00.000Z"),
    );
    assert!(row.get("USER_ID").is_none(), "physical name must not leak");
}

#[tokio::test]
async fn sqlite_crud_update_via_logical_names_writes_to_physical_columns() {
    let ds = open_in_memory().await;
    create_backend_app_user_table(&ds).await;

    let entity_map = user_entity_map();
    let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).expect("translator"));
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "app_user")
        .expect("repo")
        .with_primary_key("id")
        .expect("pk")
        .with_field_mapping_translator(translator);

    let mut seed = RowMap::new();
    seed.insert("email".to_string(), Value::from("cara@c.com"));
    seed.insert("name".to_string(), Value::from("Cara"));
    seed.insert(
        "createdAt".to_string(),
        Value::from("2026-03-03T00:00:00.000Z"),
    );
    let added = repo.add(seed).await.expect("seed via add");
    let id = added.get("id").and_then(|v| v.as_i64()).expect("id");

    let mut patch = RowMap::new();
    patch.insert("email".to_string(), Value::from("updated@a.com"));
    let updated = repo
        .update(&Value::from(id), patch)
        .await
        .expect("update")
        .expect("updated row");
    assert_eq!(
        updated.get("email").and_then(|v| v.as_str()),
        Some("updated@a.com"),
    );
    assert_eq!(updated.get("name").and_then(|v| v.as_str()), Some("Cara"));

    let raw = ds
        .query(
            "SELECT EMAIL_ADDRESS, FULL_NAME FROM app_user WHERE USER_ID = ?",
            &[Value::from(id)],
        )
        .await
        .expect("raw select");
    assert_eq!(raw.len(), 1);
    assert_eq!(
        raw[0].get("EMAIL_ADDRESS").and_then(|v| v.as_str()),
        Some("updated@a.com"),
    );
    assert_eq!(
        raw[0].get("FULL_NAME").and_then(|v| v.as_str()),
        Some("Cara")
    );
}

// why SqliteStandardRepository for converters: only StandardRepository exposes with_column_datasource_types / with_converters; it also hardcodes `id` PK and auto-injects uuid/created/updated, so the table drops USER_ID rename and adds the StandardTable trio.
struct UppercaseStringConverter;

impl TypeFieldConverter for UppercaseStringConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Sqlite
    }
    fn datasource_type(&self) -> &str {
        "string"
    }
    fn to(&self, value: &Value) -> Value {
        match value.as_str() {
            Some(s) => Value::from(s.to_uppercase()),
            None => value.clone(),
        }
    }
    fn from(&self, value: &Value) -> Value {
        match value.as_str() {
            Some(s) => Value::from(s.to_uppercase()),
            None => value.clone(),
        }
    }
}

#[tokio::test]
async fn sqlite_crud_custom_converter_override_applies_to_logical_field() {
    let ds = open_in_memory().await;
    ds.query(
        "CREATE TABLE app_user (\
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            uuid TEXT NOT NULL, \
            created TEXT NOT NULL, \
            updated TEXT NOT NULL, \
            EMAIL_ADDRESS TEXT NOT NULL, \
            FULL_NAME TEXT NOT NULL\
        )",
        &[],
    )
    .await
    .expect("create app_user (standard-repo shape)");

    let mut entity_map = EntityFieldMap::new();
    entity_map.insert("email".to_string(), "EMAIL_ADDRESS".to_string());
    entity_map.insert("name".to_string(), "FULL_NAME".to_string());
    let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).expect("translator"));

    let mut col_types: BTreeMap<String, String> = BTreeMap::new();
    col_types.insert("email".to_string(), "string".to_string());
    col_types.insert("name".to_string(), "string".to_string());

    let repo = SqliteStandardRepository::new(Arc::clone(&ds), "app_user")
        .expect("repo")
        .with_field_mapping_translator(translator)
        .with_column_datasource_types(col_types)
        .with_converters(vec![
            Arc::new(UppercaseStringConverter) as Arc<dyn TypeFieldConverter>
        ]);

    let mut row = RowMap::new();
    row.insert("email".to_string(), Value::from("dani@d.com"));
    row.insert("name".to_string(), Value::from("Dani"));
    let added = repo.add(row).await.expect("add");

    assert_eq!(
        added.get("email").and_then(|v| v.as_str()),
        Some("DANI@D.COM"),
    );
    assert_eq!(added.get("name").and_then(|v| v.as_str()), Some("DANI"));

    let id = added.get("id").and_then(|v| v.as_i64()).expect("id");
    let raw = ds
        .query(
            "SELECT EMAIL_ADDRESS, FULL_NAME FROM app_user WHERE id = ?",
            &[Value::from(id)],
        )
        .await
        .expect("raw select");
    assert_eq!(raw.len(), 1);
    assert_eq!(
        raw[0].get("EMAIL_ADDRESS").and_then(|v| v.as_str()),
        Some("DANI@D.COM"),
    );
    assert_eq!(
        raw[0].get("FULL_NAME").and_then(|v| v.as_str()),
        Some("DANI")
    );

    let fetched = repo
        .find(&Value::from(id))
        .await
        .expect("find")
        .expect("row exists");
    assert_eq!(
        fetched.get("email").and_then(|v| v.as_str()),
        Some("DANI@D.COM"),
    );
    assert_eq!(fetched.get("name").and_then(|v| v.as_str()), Some("DANI"));
}
