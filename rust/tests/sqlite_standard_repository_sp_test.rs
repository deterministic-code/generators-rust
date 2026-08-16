use deterministic::repositories::sqlite::{
    SqliteDatasource, SqliteDatasourceOptions, SqliteStandardRepository,
};
use deterministic::Datasource;
use std::sync::Arc;

async fn open_in_memory() -> Arc<SqliteDatasource> {
    let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
    ds.open().await.expect("open in-memory sqlite");
    Arc::new(ds)
}

#[tokio::test]
async fn sqlite_repo_with_stored_procs_errors_at_validate() {
    let ds = open_in_memory().await;
    let result = SqliteStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .validate();
    let err = result.expect_err("expected error: sqlite does not support stored procedures");
    let msg = format!("{err}").to_lowercase();
    assert!(
        msg.contains("sqlite does not support stored procedures"),
        "got message: {msg}"
    );
}

#[tokio::test]
async fn sqlite_repo_without_stored_procs_validates_ok() {
    let ds = open_in_memory().await;
    let repo = SqliteStandardRepository::new(Arc::clone(&ds), "user").expect("repo new");
    repo.validate().expect("validate without SP must succeed");
}

#[tokio::test]
async fn sqlite_repo_occ_without_stored_procs_errors_at_validate() {
    let ds = open_in_memory().await;
    let result = SqliteStandardRepository::new(Arc::clone(&ds), "user")
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
