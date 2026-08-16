use async_trait::async_trait;
use deterministic::repositories::sqlite::{
    SqliteCrudRepository, SqliteDatasource, SqliteDatasourceOptions,
};
use deterministic::{CrudRepository, Datasource, RepositoryError, RowMap};
use deterministic::{DataSourceMiddleware, DatabaseBackend, RewrittenQuery};
use serde_json::Value;
use std::sync::{Arc, Mutex};

async fn open_in_memory() -> Arc<SqliteDatasource> {
    let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
    ds.open().await.expect("open in-memory sqlite");
    Arc::new(ds)
}

async fn create_things_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE things (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create things table");
}

struct RecordingMiddleware {
    before_calls: Mutex<Vec<(DatabaseBackend, String, Option<Vec<Value>>)>>,
    after_calls: Mutex<Vec<AfterRecord>>,
    rewrite_to: Option<RewrittenQuery>,
}

struct AfterRecord {
    provider: DatabaseBackend,
    query: String,
    elapsed_ms: f64,
    error_message: Option<String>,
}

impl RecordingMiddleware {
    fn new() -> Self {
        Self {
            before_calls: Mutex::new(Vec::new()),
            after_calls: Mutex::new(Vec::new()),
            rewrite_to: None,
        }
    }

    fn with_rewrite(rewrite: RewrittenQuery) -> Self {
        Self {
            before_calls: Mutex::new(Vec::new()),
            after_calls: Mutex::new(Vec::new()),
            rewrite_to: Some(rewrite),
        }
    }
}

#[async_trait]
impl DataSourceMiddleware for RecordingMiddleware {
    async fn before_query(
        &self,
        provider: DatabaseBackend,
        query: &str,
        params: Option<&[Value]>,
    ) -> Option<RewrittenQuery> {
        self.before_calls.lock().unwrap().push((
            provider,
            query.to_string(),
            params.map(|p| p.to_vec()),
        ));
        self.rewrite_to.clone()
    }

    async fn after_query(
        &self,
        provider: DatabaseBackend,
        query: &str,
        _params: Option<&[Value]>,
        _results: &[Value],
        elapsed_ms: f64,
        error: Option<&RepositoryError>,
    ) {
        self.after_calls.lock().unwrap().push(AfterRecord {
            provider,
            query: query.to_string(),
            elapsed_ms,
            error_message: error.map(|e| e.to_string()),
        });
    }
}

#[tokio::test]
async fn crud_repo_without_middlewares_behaves_identically_to_before() {
    let ds = open_in_memory().await;
    create_things_table(&ds).await;
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "things").unwrap();

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    let added = repo.add(data).await.unwrap();
    assert_eq!(added.get("id").unwrap().as_i64(), Some(1));
    assert_eq!(added.get("name").unwrap().as_str(), Some("alice"));
}

#[tokio::test]
async fn crud_repo_with_middleware_fires_before_and_after_per_call() {
    let ds = open_in_memory().await;
    create_things_table(&ds).await;
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "things")
        .unwrap()
        .with_middlewares(vec![recorder.clone()]);

    let mut data = RowMap::new();
    data.insert("name".to_string(), Value::from("alice"));
    let _ = repo.add(data).await.unwrap();

    let before = recorder.before_calls.lock().unwrap();
    let after = recorder.after_calls.lock().unwrap();
    assert_eq!(before.len(), after.len(), "before/after counts must match");
    // why 1: add() is a single INSERT ... RETURNING * (was 3 calls pre-fix)
    assert_eq!(
        before.len(),
        1,
        "add() should issue exactly one query, got {}",
        before.len()
    );
    for record in after.iter() {
        assert_eq!(record.provider, DatabaseBackend::Sqlite);
        assert!(record.elapsed_ms >= 0.0, "elapsed should be non-negative");
        assert!(
            record.error_message.is_none(),
            "no error expected on happy path"
        );
    }
}

#[tokio::test]
async fn after_query_captures_error_message_when_underlying_query_fails() {
    let ds = open_in_memory().await;
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "things")
        .unwrap()
        .with_middlewares(vec![recorder.clone()]);

    let result = repo.find_all().await;
    assert!(result.is_err(), "no such table should fail");

    let after = recorder.after_calls.lock().unwrap();
    assert_eq!(after.len(), 1);
    assert!(after[0].error_message.is_some());
}

#[tokio::test]
async fn before_query_can_rewrite_sql_and_params() {
    let ds = open_in_memory().await;
    create_things_table(&ds).await;
    ds.query(
        "INSERT INTO things (name) VALUES (?)",
        &[Value::from("alice")],
    )
    .await
    .unwrap();
    ds.query(
        "INSERT INTO things (name) VALUES (?)",
        &[Value::from("bob")],
    )
    .await
    .unwrap();

    let rewriter = Arc::new(RecordingMiddleware::with_rewrite(RewrittenQuery {
        query: "SELECT * FROM things WHERE name = ?".to_string(),
        params: vec![Value::from("bob")],
    }));
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "things")
        .unwrap()
        .with_middlewares(vec![rewriter.clone()]);

    let rows = repo.find_all().await.unwrap();
    assert_eq!(rows.len(), 1, "rewritten query should return only bob");
    assert_eq!(rows[0].get("name").unwrap().as_str(), Some("bob"));
}

#[tokio::test]
async fn after_query_observes_rewritten_sql_not_original() {
    let ds = open_in_memory().await;
    create_things_table(&ds).await;

    let rewriter = Arc::new(RecordingMiddleware::with_rewrite(RewrittenQuery {
        query: "SELECT 42 AS answer".to_string(),
        params: vec![],
    }));
    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "things")
        .unwrap()
        .with_middlewares(vec![rewriter.clone()]);

    let _ = repo.find_all().await.unwrap();
    let after = rewriter.after_calls.lock().unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].query, "SELECT 42 AS answer");
}
