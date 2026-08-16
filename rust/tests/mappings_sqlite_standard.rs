use async_trait::async_trait;
use deterministic::mappings::{EntityFieldMap, FieldMappingTranslator, TypeFieldConverter};
use deterministic::repositories::sqlite::{
    SqliteDatasource, SqliteDatasourceOptions, SqliteStandardRepository,
};
use deterministic::{CrudRepository, Datasource, RepositoryError, RowMap};
use deterministic::{DataSourceMiddleware, DatabaseBackend, RewrittenQuery};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

async fn open_in_memory() -> Arc<SqliteDatasource> {
    let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
    ds.open().await.expect("open in-memory sqlite");
    Arc::new(ds)
}

async fn create_users_physical_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL, user_name TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create users");
}

async fn create_things_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE things (id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL, name TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create things");
}

#[tokio::test]
async fn standard_repo_without_mappings_or_middleware_behaves_as_before() {
    let ds = open_in_memory().await;
    create_things_table(&ds).await;
    let repo = SqliteStandardRepository::new(Arc::clone(&ds), "things").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    let added = repo.add(data).await.unwrap();
    assert_eq!(added.get("id").unwrap().as_i64(), Some(1));
    assert_eq!(added.get("name").unwrap().as_str(), Some("alice"));
    assert!(added.get("uuid").unwrap().as_str().is_some());
    assert!(added.get("created").unwrap().as_str().is_some());
}

#[tokio::test]
async fn standard_repo_with_field_mapping_translates_logical_to_physical_on_write_and_back_on_read()
{
    let ds = open_in_memory().await;
    create_users_physical_table(&ds).await;

    let mut entity_map = EntityFieldMap::new();
    entity_map.insert("username".to_string(), "user_name".to_string());
    let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).unwrap());

    let repo = SqliteStandardRepository::new(Arc::clone(&ds), "users")
        .unwrap()
        .with_field_mapping_translator(Arc::clone(&translator));

    let mut data = RowMap::new();
    data.insert("username".to_string(), Value::from("alice"));
    let added = repo.add(data).await.unwrap();

    assert_eq!(
        added.get("username").unwrap().as_str(),
        Some("alice"),
        "returned row should use logical names",
    );
    assert!(
        added.get("user_name").is_none(),
        "physical name must not leak to caller",
    );

    let found = repo
        .find(&serde_json::Value::from(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.get("username").unwrap().as_str(), Some("alice"));
}

struct RecordingMw {
    queries: Mutex<Vec<String>>,
}

#[async_trait]
impl DataSourceMiddleware for RecordingMw {
    async fn before_query(
        &self,
        _provider: DatabaseBackend,
        query: &str,
        _params: Option<&[Value]>,
    ) -> Option<RewrittenQuery> {
        self.queries.lock().unwrap().push(query.to_string());
        None
    }

    async fn after_query(
        &self,
        _provider: DatabaseBackend,
        _query: &str,
        _params: Option<&[Value]>,
        _results: &[Value],
        _elapsed_ms: f64,
        _error: Option<&RepositoryError>,
    ) {
    }
}

#[tokio::test]
async fn standard_repo_with_middleware_observes_translated_physical_sql() {
    let ds = open_in_memory().await;
    create_users_physical_table(&ds).await;

    let mut entity_map = EntityFieldMap::new();
    entity_map.insert("username".to_string(), "user_name".to_string());
    let translator = Arc::new(FieldMappingTranslator::new(Some(&entity_map)).unwrap());
    let recorder = Arc::new(RecordingMw {
        queries: Mutex::new(Vec::new()),
    });

    let repo = SqliteStandardRepository::new(Arc::clone(&ds), "users")
        .unwrap()
        .with_field_mapping_translator(translator)
        .with_middlewares(vec![recorder.clone()]);

    let mut data = RowMap::new();
    data.insert("username".to_string(), Value::from("alice"));
    let _ = repo.add(data).await.unwrap();

    let queries = recorder.queries.lock().unwrap();
    let insert_query = queries
        .iter()
        .find(|q| q.starts_with("INSERT"))
        .expect("middleware should see the INSERT");
    assert!(
        insert_query.contains("user_name"),
        "middleware should observe the PHYSICAL column name, not 'username'. got: {}",
        insert_query,
    );
    assert!(
        !insert_query.contains("username"),
        "logical 'username' must not appear in the SQL the datasource receives",
    );
}

struct UppercaseConverter;

impl TypeFieldConverter for UppercaseConverter {
    fn from_datasource(&self) -> DatabaseBackend {
        DatabaseBackend::Sqlite
    }
    fn datasource_type(&self) -> &str {
        "string"
    }
    fn from(&self, value: &Value) -> Value {
        match value.as_str() {
            Some(s) => Value::from(s.to_uppercase()),
            None => value.clone(),
        }
    }
    fn to(&self, value: &Value) -> Value {
        match value.as_str() {
            Some(s) => Value::from(s.to_lowercase()),
            None => value.clone(),
        }
    }
}

#[tokio::test]
async fn standard_repo_applies_type_field_converter_on_write_and_read() {
    let ds = open_in_memory().await;
    create_things_table(&ds).await;

    let mut column_types = BTreeMap::new();
    column_types.insert("name".to_string(), "string".to_string());

    let repo = SqliteStandardRepository::new(Arc::clone(&ds), "things")
        .unwrap()
        .with_column_datasource_types(column_types)
        .with_converters(vec![Arc::new(UppercaseConverter)]);

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("AliCe"));
    let _ = repo.add(data).await.unwrap();

    let raw_rows = ds
        .query("SELECT name FROM things WHERE id = 1", &[])
        .await
        .unwrap();
    assert_eq!(
        raw_rows[0].get("name").unwrap().as_str(),
        Some("alice"),
        "to() must have lowercased before bind",
    );

    let found = repo
        .find(&serde_json::Value::from(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        found.get("name").unwrap().as_str(),
        Some("ALICE"),
        "from() must have uppercased after fetch",
    );
}
