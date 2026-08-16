use deterministic::error::RepositoryError;
use deterministic::repositories::mysql::{
    MysqlColumnDecoder, MysqlCrudRepository, MysqlDatasource, MysqlDatasourceOptions,
    MysqlStandardRepository,
};
use deterministic::{CrudRepository, Datasource, RowMap};
use serde_json::Value;
use sqlx::mysql::MySqlRow;
use sqlx::Row as _;
use std::sync::Arc;
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

async fn drop_and_create(ds: &Arc<MysqlDatasource>, table: &str, schema: &str) {
    ds.query(&format!("DROP TABLE IF EXISTS {}", table), &[])
        .await
        .expect("drop table");
    ds.query(schema, &[]).await.expect("create table");
}

#[tokio::test]
async fn mysql_live_crud_roundtrip() {
    if docker_disabled() {
        eprintln!("SKIP_DOCKER_TESTS set — skipping mysql live test");
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "test_widget",
        "CREATE TABLE test_widget (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL)",
    )
    .await;

    let repo = MysqlCrudRepository::new(Arc::clone(&ds), "test_widget").unwrap();

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("alpha"));
    let created = repo.add(row).await.expect("insert");
    let id = created.get("id").and_then(|v| v.as_i64()).expect("id");

    let fetched = repo
        .find(&serde_json::Value::from(id))
        .await
        .expect("find")
        .expect("row");
    assert_eq!(fetched.get("name").and_then(|v| v.as_str()), Some("alpha"));

    let deleted = repo
        .delete(&serde_json::Value::from(id))
        .await
        .expect("delete");
    assert!(deleted);
}

#[tokio::test]
async fn mysql_live_standard_repository_populates_audit_columns() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "test_audited",
        "CREATE TABLE test_audited (id BIGINT AUTO_INCREMENT PRIMARY KEY, uuid VARCHAR(255) NOT NULL, name VARCHAR(255) NOT NULL, created VARCHAR(255) NOT NULL, updated VARCHAR(255) NOT NULL)",
    )
    .await;

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "test_audited").unwrap();

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("beta"));
    let created = repo.add(row).await.expect("insert");

    assert!(created.get("uuid").and_then(|v| v.as_str()).is_some());
    assert!(created.get("created").and_then(|v| v.as_str()).is_some());
    assert!(created.get("updated").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn mysql_live_tinyint_bool_column_decodes_as_value_bool() {
    // why: row_convert TINYINT/BOOLEAN branch flipped from sqlx i8→Number(0|1) to native bool→Value::Bool; this asserts the cell change end-to-end via real sqlx so the MysqlBooleanConverter's Bool-passthrough is what actually runs on read.
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "bool_test",
        "CREATE TABLE bool_test (id BIGINT AUTO_INCREMENT PRIMARY KEY, is_active TINYINT(1) NOT NULL, is_system BOOLEAN NOT NULL)",
    )
    .await;

    ds.query(
        "INSERT INTO bool_test (is_active, is_system) VALUES (?, ?)",
        &[Value::Bool(true), Value::Bool(false)],
    )
    .await
    .expect("seed bool row");

    let rows = ds
        .query("SELECT is_active, is_system FROM bool_test", &[])
        .await
        .expect("select bool row");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(
        row.get("is_active"),
        Some(&Value::Bool(true)),
        "TINYINT(1) must decode as Value::Bool(true), not Number(1)"
    );
    assert_eq!(
        row.get("is_system"),
        Some(&Value::Bool(false)),
        "BOOLEAN must decode as Value::Bool(false), not Number(0)"
    );
}

#[tokio::test]
async fn mysql_live_text_column_routes_through_text_decoder_override() {
    // why: locks the open extension point — a user-registered MysqlColumnDecoder must run BEFORE the builtin MysqlTextColumnDecoder so app code can override how text cells render without forking the library. If the builtin lookup happened first, this assertion would fail with the original string.
    if docker_disabled() {
        return;
    }
    struct ShoutingTextDecoder;
    impl MysqlColumnDecoder for ShoutingTextDecoder {
        fn decode(
            &self,
            row: &MySqlRow,
            idx: usize,
            type_name: &str,
        ) -> Result<Option<Value>, RepositoryError> {
            match type_name {
                "CHAR" | "VARCHAR" | "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" => {
                    let s: String = row.try_get(idx).map_err(RepositoryError::Sqlx)?;
                    Ok(Some(Value::String(s.to_uppercase())))
                }
                _ => Ok(None),
            }
        }
    }

    let container = start_container().await;
    let host = container.get_host().await.expect("container host");
    let port = container
        .get_host_port_ipv4(3306)
        .await
        .expect("container port");
    let url = format!("mysql://root@{}:{}/test", host, port);
    let mut ds = MysqlDatasource::new(MysqlDatasourceOptions {
        url,
        max_connections: 2,
    })
    .with_decoder(Arc::new(ShoutingTextDecoder));
    ds.open().await.expect("open mysql with custom decoder");
    let ds = Arc::new(ds);
    drop_and_create(
        &ds,
        "text_decoder_test",
        "CREATE TABLE text_decoder_test (id BIGINT AUTO_INCREMENT PRIMARY KEY, label VARCHAR(64) NOT NULL, body TEXT NOT NULL, count INT NOT NULL)",
    )
    .await;

    ds.query(
        "INSERT INTO text_decoder_test (label, body, count) VALUES (?, ?, ?)",
        &[
            Value::from("quiet"),
            Value::from("also-quiet"),
            Value::from(42),
        ],
    )
    .await
    .expect("seed row");

    let rows = ds
        .query("SELECT label, body, count FROM text_decoder_test", &[])
        .await
        .expect("select");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("label").and_then(|v| v.as_str()),
        Some("QUIET"),
        "VARCHAR must route through the custom decoder (uppercased), not the builtin Text"
    );
    assert_eq!(
        rows[0].get("body").and_then(|v| v.as_str()),
        Some("ALSO-QUIET"),
        "TEXT must route through the custom decoder too"
    );
    assert_eq!(
        rows[0].get("count").and_then(|v| v.as_i64()),
        Some(42),
        "INT must fall through to MysqlNumericColumnDecoder, not the text override"
    );
}

#[tokio::test]
async fn mysql_live_find_by_returns_matching_rows() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "test_widget",
        "CREATE TABLE test_widget (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL)",
    )
    .await;
    let repo = MysqlCrudRepository::new(Arc::clone(&ds), "test_widget").unwrap();

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("findable"));
    repo.add(row).await.expect("insert");

    let matches = repo
        .find_by("name", &Value::from("findable"))
        .await
        .expect("find_by");
    assert_eq!(matches.len(), 1);
    assert_eq!(
        matches[0].get("name").and_then(|v| v.as_str()),
        Some("findable")
    );
}
