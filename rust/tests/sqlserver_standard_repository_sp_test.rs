use async_trait::async_trait;
use deterministic::repositories::sqlserver::{
    SqlserverDatasource, SqlserverDatasourceOptions, SqlserverStandardRepository,
};
use deterministic::{CrudRepository, RepositoryError, RowMap};
use deterministic::{DataSourceMiddleware, DatabaseBackend, RewrittenQuery};
use serde_json::Value;
use std::sync::{Arc, Mutex};

struct RecordingMiddleware {
    queries: Mutex<Vec<String>>,
}

impl RecordingMiddleware {
    fn new() -> Self {
        Self {
            queries: Mutex::new(Vec::new()),
        }
    }

    fn snapshot(&self) -> Vec<String> {
        self.queries.lock().unwrap().clone()
    }
}

#[async_trait]
impl DataSourceMiddleware for RecordingMiddleware {
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

/// The sqlserver stub's open() returns Unimplemented until tiberius lands; these tests pin SQL generation via the recording middleware, which never needs a live connection.
async fn open_datasource() -> Arc<SqlserverDatasource> {
    Arc::new(SqlserverDatasource::new(
        SqlserverDatasourceOptions::default(),
    ))
}

#[tokio::test]
async fn sqlserver_repo_without_stored_procs_uses_inline_select() {
    let ds = open_datasource().await;
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = SqlserverStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find(&serde_json::Value::from(1)).await;

    let calls = recorder.snapshot();
    assert!(
        !calls.is_empty(),
        "middleware should record at least one call"
    );
    let first = &calls[0];
    assert!(
        first.to_uppercase().contains("SELECT"),
        "expected SELECT, got: {first}"
    );
    let lower = first.to_lowercase();
    assert!(!lower.contains("exec find_user"));
}

#[tokio::test]
async fn sqlserver_repo_with_stored_procs_issues_exec_create_user_without_output() {
    let ds = open_datasource().await;
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = SqlserverStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("Alice"));
    let _ = repo.add(row).await;

    let calls = recorder.snapshot();
    assert!(
        !calls.is_empty(),
        "middleware should record at least one call"
    );
    let first_low = calls[0].to_lowercase();
    assert!(
        first_low.contains("exec create_user") || first_low.contains("execute create_user"),
        "expected EXEC create_user, got: {}",
        calls[0]
    );
    assert!(
        !first_low.contains("output")
            && !first_low.contains("@out_id")
            && !first_low.contains("@out_affected"),
        "must not pass OUTPUT or @out_* placeholders, got: {}",
        calls[0]
    );
    assert!(
        !calls
            .iter()
            .any(|q| q.to_lowercase().contains("select @out_id")
                || q.to_lowercase().contains("select @out_affected")),
        "must not round-trip via SELECT @out_id/@out_affected, got queries: {:?}",
        calls
    );
}

#[tokio::test]
async fn sqlserver_repo_with_stored_procs_find_uses_exec_find_user() {
    let ds = open_datasource().await;
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = SqlserverStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find(&serde_json::Value::from(1)).await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    let first_low = calls[0].to_lowercase();
    assert!(
        first_low.contains("exec find_user"),
        "expected EXEC find_user, got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn sqlserver_repo_with_stored_procs_find_all_uses_exec_find_users() {
    let ds = open_datasource().await;
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = SqlserverStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find_all().await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    let first_low = calls[0].to_lowercase();
    assert!(
        first_low.contains("exec find_users"),
        "expected EXEC find_users, got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn sqlserver_repo_with_stored_procs_find_by_uses_exec_per_field() {
    let ds = open_datasource().await;
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = SqlserverStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find_by("email", &Value::from("a@b.c")).await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    let first_low = calls[0].to_lowercase();
    assert!(
        first_low.contains("exec find_user_by_email"),
        "expected EXEC find_user_by_email, got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn sqlserver_repo_occ_without_stored_procs_errors_at_validate() {
    let ds = open_datasource().await;
    let result = SqlserverStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_optimistic_concurrency(true)
        .validate();
    let err = result.expect_err("expected error when OCC is on without SP");
    let msg = format!("{err}");
    assert!(
        msg.contains("useOptimisticConcurrency requires useStoredProcedures"),
        "got message: {msg}"
    );
}

#[tokio::test]
#[ignore]
async fn sqlserver_sp_lifecycle_parity_against_testcontainer() {
    // TODO(GATE 15): no sqlserver testcontainer harness in rust/ — add when tiberius wiring lands
}

#[tokio::test]
#[ignore]
async fn sqlserver_sp_occ_update_conflict_against_testcontainer() {
    // TODO(GATE 15): no sqlserver testcontainer harness in rust/ — add when tiberius wiring lands
}

#[tokio::test]
#[ignore]
async fn sqlserver_sp_occ_delete_conflict_against_testcontainer() {
    // TODO(GATE 15): no sqlserver testcontainer harness in rust/ — add when tiberius wiring lands
}
