use async_trait::async_trait;
use deterministic::repositories::postgres::{
    PostgresDatasource, PostgresDatasourceOptions, PostgresStandardRepository,
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

fn closed_datasource() -> Arc<PostgresDatasource> {
    Arc::new(PostgresDatasource::new(PostgresDatasourceOptions {
        url: "postgres://unused".to_string(),
        max_connections: 1,
    }))
}

#[tokio::test]
async fn postgres_repo_without_stored_procs_issues_insert() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_middlewares(vec![recorder.clone()]);

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("Alice"));
    let _ = repo.add(row).await;

    let calls = recorder.snapshot();
    assert!(
        !calls.is_empty(),
        "middleware should record at least one call"
    );
    let first = &calls[0];
    assert!(
        first.contains("INSERT INTO"),
        "expected INSERT, got: {first}"
    );
    assert!(!first.to_lowercase().contains("call create_user"));
}

#[tokio::test]
async fn postgres_repo_with_stored_procs_issues_select_create_user_function() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
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
        first_low.contains("select * from create_user("),
        "expected SELECT * FROM create_user(...), got: {}",
        calls[0]
    );
    assert!(
        !first_low.contains("call create_user")
            && !first_low.contains("inout")
            && !first_low.contains("null)"),
        "must not use CALL or any OUT/INOUT plumbing, got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn postgres_repo_with_stored_procs_find_uses_function() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find(&serde_json::Value::from(1)).await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    assert!(
        calls[0].contains("find_user("),
        "expected find_user(...), got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn postgres_repo_with_stored_procs_find_all_uses_plural_function() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find_all().await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    assert!(
        calls[0].contains("find_users()"),
        "expected find_users(), got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn postgres_repo_with_stored_procs_find_by_uses_per_field_function() {
    let ds = closed_datasource();
    let recorder = Arc::new(RecordingMiddleware::new());
    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_middlewares(vec![recorder.clone()]);

    let _ = repo.find_by("email", &Value::from("a@b.c")).await;
    let calls = recorder.snapshot();
    assert!(!calls.is_empty());
    assert!(
        calls[0].contains("find_user_by_email"),
        "expected find_user_by_email, got: {}",
        calls[0]
    );
}

#[tokio::test]
async fn postgres_repo_occ_without_stored_procs_panics_at_build() {
    let ds = closed_datasource();
    let result = PostgresStandardRepository::new(Arc::clone(&ds), "user")
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
async fn postgres_sp_lifecycle_parity_against_testcontainer() {
    if docker_disabled() {
        return;
    }
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default()
        .start()
        .await
        .expect("start postgres testcontainer");
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);

    let mut ds = PostgresDatasource::new(PostgresDatasourceOptions {
        url,
        max_connections: 2,
    });
    ds.open().await.expect("open postgres");
    let ds = Arc::new(ds);

    ds.query("DROP TABLE IF EXISTS \"user\" CASCADE", &[])
        .await
        .expect("drop table");
    ds.query(
        "CREATE TABLE \"user\" (id BIGSERIAL PRIMARY KEY, uuid TEXT NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create table");
    apply_postgres_procs(&ds).await;

    let sp_repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("sp repo")
        .with_stored_procedures(true);
    let inline_repo =
        PostgresStandardRepository::new(Arc::clone(&ds), "user").expect("inline repo");

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
async fn postgres_sp_occ_update_happy_path_against_testcontainer() {
    if docker_disabled() {
        return;
    }
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default().start().await.expect("start pg");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    let mut ds = PostgresDatasource::new(PostgresDatasourceOptions {
        url,
        max_connections: 2,
    });
    ds.open().await.expect("open");
    let ds = Arc::new(ds);
    ds.query("DROP TABLE IF EXISTS \"user\" CASCADE", &[])
        .await
        .unwrap();
    ds.query(
        "CREATE TABLE \"user\" (id BIGSERIAL PRIMARY KEY, uuid TEXT NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL)",
        &[],
    ).await.unwrap();
    apply_postgres_procs(&ds).await;

    let oc_repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("oc repo")
        .with_stored_procedures(true)
        .with_optimistic_concurrency(true);

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("Dan"));
    row.insert("email".to_string(), Value::from("dan@d.com"));
    let added = oc_repo.add(row).await.expect("add");
    let fresh_updated = added
        .get("updated")
        .and_then(|v| v.as_str())
        .expect("updated")
        .to_string();
    let id = added.get("id").and_then(|v| v.as_i64()).expect("id");

    let mut update = RowMap::new();
    update.insert("name".to_string(), Value::from("Dan2"));
    let updated = oc_repo
        .update_with_expected(id, update, &fresh_updated)
        .await
        .expect("update_with_expected happy")
        .expect("updated row");
    assert_eq!(updated.get("name").and_then(|v| v.as_str()), Some("Dan2"));
}

#[tokio::test]
#[ignore]
async fn postgres_sp_occ_update_conflict_against_testcontainer() {
    if docker_disabled() {
        return;
    }
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default().start().await.expect("start pg");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    let mut ds = PostgresDatasource::new(PostgresDatasourceOptions {
        url,
        max_connections: 2,
    });
    ds.open().await.expect("open");
    let ds = Arc::new(ds);
    ds.query("DROP TABLE IF EXISTS \"user\" CASCADE", &[])
        .await
        .unwrap();
    ds.query(
        "CREATE TABLE \"user\" (id BIGSERIAL PRIMARY KEY, uuid TEXT NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL)",
        &[],
    ).await.unwrap();
    apply_postgres_procs(&ds).await;

    let oc_repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
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
async fn postgres_sp_occ_delete_conflict_against_testcontainer() {
    if docker_disabled() {
        return;
    }
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default().start().await.expect("start pg");
    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    let mut ds = PostgresDatasource::new(PostgresDatasourceOptions {
        url,
        max_connections: 2,
    });
    ds.open().await.expect("open");
    let ds = Arc::new(ds);
    ds.query("DROP TABLE IF EXISTS \"user\" CASCADE", &[])
        .await
        .unwrap();
    ds.query(
        "CREATE TABLE \"user\" (id BIGSERIAL PRIMARY KEY, uuid TEXT NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL)",
        &[],
    ).await.unwrap();
    apply_postgres_procs(&ds).await;

    let oc_repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
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

async fn apply_postgres_procs(ds: &Arc<PostgresDatasource>) {
    let procs: Vec<&str> = vec![
        r#"CREATE OR REPLACE FUNCTION create_user(
            p_uuid    TEXT,
            p_name    TEXT,
            p_email   TEXT,
            p_created TEXT,
            p_updated TEXT
        ) RETURNS BIGINT LANGUAGE plpgsql AS $$
        DECLARE new_id BIGINT;
        BEGIN
            INSERT INTO "user" (uuid, name, email, created, updated)
            VALUES (p_uuid, p_name, p_email, p_created, p_updated)
            RETURNING id INTO new_id;
            RETURN new_id;
        END;
        $$"#,
        r#"CREATE OR REPLACE FUNCTION find_user(p_id BIGINT) RETURNS SETOF "user"
            LANGUAGE sql AS $$ SELECT * FROM "user" WHERE id = p_id; $$"#,
        r#"CREATE OR REPLACE FUNCTION find_users() RETURNS SETOF "user"
            LANGUAGE sql AS $$ SELECT * FROM "user" ORDER BY id; $$"#,
        r#"CREATE OR REPLACE FUNCTION find_user_by_email(p_email TEXT) RETURNS SETOF "user"
            LANGUAGE sql AS $$ SELECT * FROM "user" WHERE email = p_email; $$"#,
        r#"CREATE OR REPLACE FUNCTION update_user(
            p_id          BIGINT,
            p_name        TEXT,
            p_email       TEXT,
            p_new_updated TEXT
        ) RETURNS INT LANGUAGE plpgsql AS $$
        DECLARE affected INT;
        BEGIN
            UPDATE "user"
            SET name    = COALESCE(p_name, name),
                email   = COALESCE(p_email, email),
                updated = p_new_updated
            WHERE id = p_id;
            GET DIAGNOSTICS affected = ROW_COUNT;
            RETURN affected;
        END;
        $$"#,
        r#"CREATE OR REPLACE FUNCTION update_user_optimistic_concurrency(
            p_id               BIGINT,
            p_expected_updated TEXT,
            p_name             TEXT,
            p_email            TEXT,
            p_new_updated      TEXT
        ) RETURNS INT LANGUAGE plpgsql AS $$
        DECLARE affected INT;
        BEGIN
            UPDATE "user"
            SET name    = COALESCE(p_name, name),
                email   = COALESCE(p_email, email),
                updated = p_new_updated
            WHERE id = p_id AND updated = p_expected_updated;
            GET DIAGNOSTICS affected = ROW_COUNT;
            RETURN affected;
        END;
        $$"#,
        r#"CREATE OR REPLACE FUNCTION delete_user(p_id BIGINT) RETURNS INT LANGUAGE plpgsql AS $$
        DECLARE affected INT;
        BEGIN
            DELETE FROM "user" WHERE id = p_id;
            GET DIAGNOSTICS affected = ROW_COUNT;
            RETURN affected;
        END;
        $$"#,
        r#"CREATE OR REPLACE FUNCTION delete_user_optimistic_concurrency(
            p_id               BIGINT,
            p_expected_updated TEXT
        ) RETURNS INT LANGUAGE plpgsql AS $$
        DECLARE affected INT;
        BEGIN
            DELETE FROM "user"
            WHERE id = p_id AND updated = p_expected_updated;
            GET DIAGNOSTICS affected = ROW_COUNT;
            RETURN affected;
        END;
        $$"#,
    ];
    for sql in procs {
        ds.query(sql, &[]).await.expect("apply proc");
    }
}
