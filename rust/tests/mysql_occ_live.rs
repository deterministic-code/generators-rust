use deterministic::repositories::mysql::{
    MysqlDatasource, MysqlDatasourceOptions, MysqlStandardRepository,
};
use deterministic::{CrudRepository, Datasource, RepositoryError, RowMap};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::mysql::Mysql;

fn docker_disabled() -> bool {
    std::env::var("SKIP_DOCKER_TESTS").is_ok()
}

async fn start_container() -> ContainerAsync<Mysql> {
    Mysql::default()
        .start()
        .await
        .expect("start mysql testcontainer (is docker running?)")
}

async fn open_datasource(container: &ContainerAsync<Mysql>) -> Arc<MysqlDatasource> {
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(3306)
        .await
        .expect("container port");
    let url = format!("mysql://root@{}:{}/test", host, port);
    let mut ds = MysqlDatasource::new(MysqlDatasourceOptions {
        url,
        max_connections: 2,
    });
    ds.open().await.expect("open mysql");
    Arc::new(ds)
}

async fn setup_user_table(ds: &Arc<MysqlDatasource>) {
    ds.query("DROP TABLE IF EXISTS `user`", &[])
        .await
        .expect("drop user table");
    ds.query(
        "CREATE TABLE `user` (id BIGINT AUTO_INCREMENT PRIMARY KEY, uuid VARCHAR(255) NOT NULL, name VARCHAR(255) NOT NULL, email VARCHAR(255) NOT NULL, created VARCHAR(255) NOT NULL, updated VARCHAR(255) NOT NULL)",
        &[],
    )
    .await
    .expect("create user table");
    apply_mysql_procs(ds).await;
}

async fn seed_one(repo: &MysqlStandardRepository, name: &str, email: &str) -> (i64, String) {
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
async fn mysql_occ_off_regular_update_succeeds_without_etag() {
    if docker_disabled() {
        eprintln!("SKIP_DOCKER_TESTS set — skipping mysql OCC live test");
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user").expect("repo new");
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
async fn mysql_occ_update_with_expected_errors_when_occ_disabled() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user").expect("repo new");
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
async fn mysql_occ_update_with_current_etag_succeeds_and_bumps_updated() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
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
async fn mysql_occ_update_with_stale_etag_returns_conflict_and_row_unchanged() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
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
async fn mysql_occ_delete_with_stale_etag_returns_conflict_and_row_remains() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
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
async fn mysql_occ_delete_with_current_etag_succeeds_and_row_gone() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
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
async fn mysql_occ_two_client_race_first_winner_second_loser() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    setup_user_table(&ds).await;

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "user")
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
