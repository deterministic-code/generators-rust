use async_trait::async_trait;
use deterministic::repositories::mysql::{
    MysqlDatasource, MysqlDatasourceOptions, MysqlStandardRepository,
};
use deterministic::{CrudRepository, Datasource, RepositoryError, RowMap};
use deterministic::{DataSourceMiddleware, DatabaseBackend, RewrittenQuery};
use serde_json::Value;
use std::sync::{Arc, Mutex};

fn docker_disabled() -> bool {
    std::env::var("SKIP_DOCKER_TESTS").is_ok()
}

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

fn closed_datasource() -> Arc<MysqlDatasource> {
    Arc::new(MysqlDatasource::new(MysqlDatasourceOptions {
        url: "mysql://unused".to_string(),
        max_connections: 1,
    }))
}

#[tokio::test]
async fn mysql_repo_without_stored_procs_uses_inline_select() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
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
    assert!(!first.to_lowercase().contains("call find_user"));
}

#[tokio::test]
async fn mysql_repo_with_stored_procs_issues_call_create_user_without_out_placeholder() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
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
    let first = calls[0].to_lowercase();
    assert!(
        first.contains("call create_user"),
        "expected CALL create_user, got: {}",
        calls[0]
    );
    assert!(
        !first.contains("@out_id") && !first.contains("@out_affected"),
        "must not pass @out_id/@out_affected placeholders, got: {}",
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
async fn mysql_repo_with_stored_procs_find_uses_call_find_user() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find(&serde_json::Value::from(1)).await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    let first_low = calls[0].to_lowercase();
    assert!(
        first_low.contains("call find_user("),
        "expected CALL find_user(...), got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn mysql_repo_with_stored_procs_find_all_uses_call_find_users() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find_all().await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    let first_low = calls[0].to_lowercase();
    assert!(
        first_low.contains("call find_users()"),
        "expected CALL find_users(), got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn mysql_repo_with_stored_procs_find_by_uses_per_field_proc() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find_by("email", &Value::from("a@b.c")).await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    let first_low = calls[0].to_lowercase();
    assert!(
        first_low.contains("call find_user_by_email"),
        "expected CALL find_user_by_email, got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn mysql_repo_occ_without_stored_procs_errors_at_validate() {
    let ds = closed_datasource();
    let result = MysqlStandardRepository::new(Arc::clone(&ds), "user")
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
async fn mysql_sp_lifecycle_parity_against_testcontainer() {
    if docker_disabled() {
        return;
    }
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::mysql::Mysql;

    let container = Mysql::default().start().await.expect("start mysql");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(3306).await.expect("port");
    let url = format!("mysql://root@{}:{}/test", host, port);
    let mut ds = MysqlDatasource::new(MysqlDatasourceOptions {
        url,
        max_connections: 2,
    });
    ds.open().await.expect("open mysql");
    let ds = Arc::new(ds);

    ds.query("DROP TABLE IF EXISTS `user`", &[]).await.unwrap();
    ds.query(
        "CREATE TABLE `user` (id BIGINT AUTO_INCREMENT PRIMARY KEY, uuid VARCHAR(255) NOT NULL, name VARCHAR(255) NOT NULL, email VARCHAR(255) NOT NULL, created VARCHAR(255) NOT NULL, updated VARCHAR(255) NOT NULL)",
        &[],
    )
    .await
    .expect("create table");
    apply_mysql_procs(&ds).await;

    let sp_repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
        .expect("sp repo")
        .with_stored_procedures(true);
    let inline_repo = MysqlStandardRepository::new(Arc::clone(&ds), "user").expect("inline repo");

    let mut sp_row = RowMap::new();
    sp_row.insert("name".to_string(), Value::from("Alice"));
    sp_row.insert("email".to_string(), Value::from("alice@a.com"));
    let sp_added = sp_repo.add(sp_row).await.expect("sp add");

    let mut inline_row = RowMap::new();
    inline_row.insert("name".to_string(), Value::from("Bob"));
    inline_row.insert("email".to_string(), Value::from("bob@b.com"));
    let inline_added = inline_repo.add(inline_row).await.expect("inline add");

    let sp_keys: Vec<&String> = sp_added.keys().collect();
    let inline_keys: Vec<&String> = inline_added.keys().collect();
    assert_eq!(sp_keys, inline_keys, "SP/inline RowMap shapes must match");

    let found = sp_repo
        .find_by("email", &Value::from("alice@a.com"))
        .await
        .expect("findBy");
    assert_eq!(found.len(), 1);
}

#[tokio::test]
#[ignore]
async fn mysql_sp_occ_update_conflict_against_testcontainer() {
    if docker_disabled() {
        return;
    }
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::mysql::Mysql;

    let container = Mysql::default().start().await.expect("start mysql");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(3306).await.expect("port");
    let url = format!("mysql://root@{}:{}/test", host, port);
    let mut ds = MysqlDatasource::new(MysqlDatasourceOptions {
        url,
        max_connections: 2,
    });
    ds.open().await.expect("open");
    let ds = Arc::new(ds);
    ds.query("DROP TABLE IF EXISTS `user`", &[]).await.unwrap();
    ds.query(
        "CREATE TABLE `user` (id BIGINT AUTO_INCREMENT PRIMARY KEY, uuid VARCHAR(255) NOT NULL, name VARCHAR(255) NOT NULL, email VARCHAR(255) NOT NULL, created VARCHAR(255) NOT NULL, updated VARCHAR(255) NOT NULL)",
        &[],
    ).await.unwrap();
    apply_mysql_procs(&ds).await;

    let oc_repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
        .expect("oc repo")
        .with_stored_procedures(true)
        .with_optimistic_concurrency(true);

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("Eve"));
    row.insert("email".to_string(), Value::from("eve@e.com"));
    let added = oc_repo.add(row).await.expect("add");
    let id = added.get("id").and_then(|v| v.as_i64()).expect("id");

    let mut update = RowMap::new();
    update.insert("name".to_string(), Value::from("Eve2"));
    let result = oc_repo
        .update_with_expected(id, update, "1970-01-01T00:00:00.000Z")
        .await;
    match result {
        Err(RepositoryError::ConcurrencyConflict(_)) => {}
        other => panic!("expected ConcurrencyConflict, got {other:?}"),
    }
}

#[tokio::test]
#[ignore]
async fn mysql_sp_occ_delete_conflict_against_testcontainer() {
    if docker_disabled() {
        return;
    }
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::mysql::Mysql;

    let container = Mysql::default().start().await.expect("start mysql");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(3306).await.expect("port");
    let url = format!("mysql://root@{}:{}/test", host, port);
    let mut ds = MysqlDatasource::new(MysqlDatasourceOptions {
        url,
        max_connections: 2,
    });
    ds.open().await.expect("open");
    let ds = Arc::new(ds);
    ds.query("DROP TABLE IF EXISTS `user`", &[]).await.unwrap();
    ds.query(
        "CREATE TABLE `user` (id BIGINT AUTO_INCREMENT PRIMARY KEY, uuid VARCHAR(255) NOT NULL, name VARCHAR(255) NOT NULL, email VARCHAR(255) NOT NULL, created VARCHAR(255) NOT NULL, updated VARCHAR(255) NOT NULL)",
        &[],
    ).await.unwrap();
    apply_mysql_procs(&ds).await;

    let oc_repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
        .expect("oc repo")
        .with_stored_procedures(true)
        .with_optimistic_concurrency(true);

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("Faye"));
    row.insert("email".to_string(), Value::from("faye@f.com"));
    let added = oc_repo.add(row).await.expect("add");
    let id = added.get("id").and_then(|v| v.as_i64()).expect("id");

    let result = oc_repo
        .delete_with_expected(id, "1970-01-01T00:00:00.000Z")
        .await;
    match result {
        Err(RepositoryError::ConcurrencyConflict(_)) => {}
        other => panic!("expected ConcurrencyConflict on delete, got {other:?}"),
    }
}

async fn apply_mysql_procs(ds: &Arc<MysqlDatasource>) {
    let procs: Vec<&str> = vec![
        "DROP PROCEDURE IF EXISTS create_user",
        r#"CREATE PROCEDURE create_user(
            IN p_uuid    VARCHAR(255),
            IN p_name    VARCHAR(255),
            IN p_email   VARCHAR(255),
            IN p_created VARCHAR(255),
            IN p_updated VARCHAR(255)
        )
        BEGIN
            INSERT INTO `user` (uuid, name, email, created, updated)
            VALUES (p_uuid, p_name, p_email, p_created, p_updated);
            SELECT LAST_INSERT_ID() AS id;
        END"#,
        "DROP PROCEDURE IF EXISTS find_user",
        r#"CREATE PROCEDURE find_user(IN p_id BIGINT)
        BEGIN
            SELECT * FROM `user` WHERE id = p_id;
        END"#,
        "DROP PROCEDURE IF EXISTS find_users",
        r#"CREATE PROCEDURE find_users()
        BEGIN
            SELECT * FROM `user` ORDER BY id;
        END"#,
        "DROP PROCEDURE IF EXISTS find_user_by_email",
        r#"CREATE PROCEDURE find_user_by_email(IN p_email VARCHAR(255))
        BEGIN
            SELECT * FROM `user` WHERE email = p_email;
        END"#,
        "DROP PROCEDURE IF EXISTS update_user",
        r#"CREATE PROCEDURE update_user(
            IN p_id          BIGINT,
            IN p_name        VARCHAR(255),
            IN p_email       VARCHAR(255),
            IN p_new_updated VARCHAR(255)
        )
        BEGIN
            UPDATE `user`
            SET name    = COALESCE(p_name, name),
                email   = COALESCE(p_email, email),
                updated = p_new_updated
            WHERE id = p_id;
            SELECT ROW_COUNT() AS affected;
        END"#,
        "DROP PROCEDURE IF EXISTS update_user_optimistic_concurrency",
        r#"CREATE PROCEDURE update_user_optimistic_concurrency(
            IN p_id               BIGINT,
            IN p_expected_updated VARCHAR(255),
            IN p_name             VARCHAR(255),
            IN p_email            VARCHAR(255),
            IN p_new_updated      VARCHAR(255)
        )
        BEGIN
            UPDATE `user`
            SET name    = COALESCE(p_name, name),
                email   = COALESCE(p_email, email),
                updated = p_new_updated
            WHERE id = p_id AND updated = p_expected_updated;
            SELECT ROW_COUNT() AS affected;
        END"#,
        "DROP PROCEDURE IF EXISTS delete_user",
        r#"CREATE PROCEDURE delete_user(IN p_id BIGINT)
        BEGIN
            DELETE FROM `user` WHERE id = p_id;
            SELECT ROW_COUNT() AS affected;
        END"#,
        "DROP PROCEDURE IF EXISTS delete_user_optimistic_concurrency",
        r#"CREATE PROCEDURE delete_user_optimistic_concurrency(
            IN p_id               BIGINT,
            IN p_expected_updated VARCHAR(255)
        )
        BEGIN
            DELETE FROM `user`
            WHERE id = p_id AND updated = p_expected_updated;
            SELECT ROW_COUNT() AS affected;
        END"#,
    ];
    for sql in procs {
        ds.execute_unprepared(sql).await.expect("apply proc");
    }
}
