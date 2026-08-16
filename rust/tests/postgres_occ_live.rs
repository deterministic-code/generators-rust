use deterministic::repositories::postgres::{
    PostgresDatasource, PostgresDatasourceOptions, PostgresStandardRepository,
};
use deterministic::{CrudRepository, Datasource, RepositoryError, RowMap};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

fn docker_disabled() -> bool {
    std::env::var("SKIP_DOCKER_TESTS").is_ok()
}

async fn start_container() -> ContainerAsync<Postgres> {
    Postgres::default()
        .start()
        .await
        .expect("start postgres testcontainer (is docker running?)")
}

async fn open_datasource(container: &ContainerAsync<Postgres>) -> Arc<PostgresDatasource> {
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
    Arc::new(ds)
}

async fn setup_user_table(ds: &Arc<PostgresDatasource>) {
    ds.query("DROP TABLE IF EXISTS \"user\" CASCADE", &[])
        .await
        .expect("drop user table");
    ds.query(
        "CREATE TABLE \"user\" (id BIGSERIAL PRIMARY KEY, uuid TEXT NOT NULL, name TEXT NOT NULL, email TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create user table");
    apply_postgres_procs(ds).await;
}

async fn seed_one(repo: &PostgresStandardRepository, name: &str, email: &str) -> (i64, String) {
    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from(name));
    row.insert("email".to_string(), Value::from(email));
    let added = repo.add(row).await.expect("seed add");
    let id = added.get("id").and_then(|v| v.as_i64()).expect("seed id");
    let updated = added
        .get("updated")
        .and_then(|v| v.as_str())
        .expect("seed updated")
        .to_string();
    (id, updated)
}

#[tokio::test]
async fn postgres_occ_off_regular_update_succeeds_without_etag() {
    if docker_disabled() {
        eprintln!("SKIP_DOCKER_TESTS set — skipping postgres OCC live test");
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user").expect("repo new");
    let (id, _etag) = seed_one(&repo, "Alpha", "alpha@a.com").await;

    let mut update = RowMap::new();
    update.insert("name".to_string(), Value::from("Beta"));
    let updated = repo
        .update(&Value::from(id), update)
        .await
        .expect("regular update")
        .expect("row returned");
    assert_eq!(updated.get("name").and_then(|v| v.as_str()), Some("Beta"));
}

#[tokio::test]
async fn postgres_occ_update_with_expected_errors_when_occ_disabled() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user").expect("repo new");
    let (id, _etag) = seed_one(&repo, "Alpha", "alpha@a.com").await;

    let mut update = RowMap::new();
    update.insert("name".to_string(), Value::from("Beta"));
    let err = repo
        .update_with_expected(id, update, "any-etag")
        .await
        .expect_err("must reject update_with_expected when OCC off");
    assert!(
        matches!(err, RepositoryError::InvalidConfig(_)),
        "expected InvalidConfig, got: {err:?}"
    );

    let derr = repo
        .delete_with_expected(id, "any-etag")
        .await
        .expect_err("must reject delete_with_expected when OCC off");
    assert!(
        matches!(derr, RepositoryError::InvalidConfig(_)),
        "expected InvalidConfig, got: {derr:?}"
    );
}

#[tokio::test]
async fn postgres_occ_update_with_current_etag_succeeds_and_bumps_updated() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_optimistic_concurrency(true);

    let (id, etag) = seed_one(&repo, "Alpha", "alpha@a.com").await;
    // millisecond-resolution now_iso() needs a gap so the new etag differs
    tokio::time::sleep(Duration::from_millis(5)).await;

    let mut update = RowMap::new();
    update.insert("name".to_string(), Value::from("Beta"));
    let updated = repo
        .update_with_expected(id, update, &etag)
        .await
        .expect("update_with_expected happy")
        .expect("row returned");
    assert_eq!(updated.get("name").and_then(|v| v.as_str()), Some("Beta"));
    let new_etag = updated
        .get("updated")
        .and_then(|v| v.as_str())
        .expect("new updated");
    assert_ne!(new_etag, etag, "updated etag must bump");
}

#[tokio::test]
async fn postgres_occ_update_with_stale_etag_returns_conflict_and_row_unchanged() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_optimistic_concurrency(true);

    let (id, _etag) = seed_one(&repo, "Alpha", "alpha@a.com").await;

    let mut update = RowMap::new();
    update.insert("name".to_string(), Value::from("Beta"));
    let err = repo
        .update_with_expected(id, update, "1970-01-01T00:00:00.000Z")
        .await
        .expect_err("stale etag must conflict");
    assert!(
        matches!(err, RepositoryError::ConcurrencyConflict(_)),
        "expected ConcurrencyConflict, got: {err:?}"
    );

    let row = repo
        .find(&Value::from(id))
        .await
        .expect("find")
        .expect("row still present");
    assert_eq!(row.get("name").and_then(|v| v.as_str()), Some("Alpha"));
}

#[tokio::test]
async fn postgres_occ_delete_with_stale_etag_returns_conflict_and_row_remains() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_optimistic_concurrency(true);

    let (id, _etag) = seed_one(&repo, "Alpha", "alpha@a.com").await;

    let err = repo
        .delete_with_expected(id, "1970-01-01T00:00:00.000Z")
        .await
        .expect_err("stale etag must conflict on delete");
    assert!(
        matches!(err, RepositoryError::ConcurrencyConflict(_)),
        "expected ConcurrencyConflict, got: {err:?}"
    );

    let row = repo.find(&Value::from(id)).await.expect("find");
    assert!(row.is_some(), "row must remain after failed delete");
}

#[tokio::test]
async fn postgres_occ_delete_with_current_etag_succeeds_and_row_gone() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_optimistic_concurrency(true);

    let (id, etag) = seed_one(&repo, "Alpha", "alpha@a.com").await;

    let ok = repo
        .delete_with_expected(id, &etag)
        .await
        .expect("delete_with_expected happy");
    assert!(ok);

    let row = repo.find(&Value::from(id)).await.expect("find");
    assert!(row.is_none(), "row must be gone after successful delete");
}

#[tokio::test]
async fn postgres_occ_two_client_race_first_winner_second_loser() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "user")
        .expect("repo new")
        .with_stored_procedures(true)
        .with_optimistic_concurrency(true);

    let (id, shared_etag) = seed_one(&repo, "Alpha", "alpha@a.com").await;
    tokio::time::sleep(Duration::from_millis(5)).await;

    let mut winner = RowMap::new();
    winner.insert("name".to_string(), Value::from("WinnerWrite"));
    let _ = repo
        .update_with_expected(id, winner, &shared_etag)
        .await
        .expect("first writer wins")
        .expect("row returned");

    let mut loser = RowMap::new();
    loser.insert("name".to_string(), Value::from("LoserWrite"));
    let err = repo
        .update_with_expected(id, loser, &shared_etag)
        .await
        .expect_err("second writer must lose");
    assert!(
        matches!(err, RepositoryError::ConcurrencyConflict(_)),
        "expected ConcurrencyConflict, got: {err:?}"
    );

    let row = repo
        .find(&Value::from(id))
        .await
        .expect("find")
        .expect("row");
    assert_eq!(
        row.get("name").and_then(|v| v.as_str()),
        Some("WinnerWrite"),
        "winner's write must persist"
    );
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
