use deterministic::repositories::mysql::{
    MysqlDatasource, MysqlDatasourceOptions, MysqlStandardRepository,
};
use deterministic::{CrudRepository, Datasource, RowMap};
use serde_json::Value;
use std::sync::Arc;

fn docker_disabled() -> bool {
    std::env::var("SKIP_DOCKER_TESTS").is_ok()
}

#[tokio::test]
#[ignore]
async fn mysql_datetime_column_round_trips_through_repo_add_and_find() {
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

    ds.query("DROP TABLE IF EXISTS `audit_row`", &[])
        .await
        .unwrap();
    ds.query(
        "CREATE TABLE `audit_row` (\
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY, \
            `uuid` VARCHAR(36) NOT NULL UNIQUE DEFAULT (UUID()), \
            `name` VARCHAR(64) NOT NULL, \
            `created` DATETIME NOT NULL, \
            `updated` DATETIME NOT NULL\
        )",
        &[],
    )
    .await
    .expect("create table");

    let repo = MysqlStandardRepository::new(Arc::clone(&ds), "audit_row").expect("repo");

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("alice"));
    let added = repo
        .add(row)
        .await
        .expect("add must accept DATETIME stamping");

    let created = added
        .get("created")
        .and_then(|v| v.as_str())
        .expect("created column must decode as String (DATETIME → ISO string)");
    assert!(
        created.contains('-') && created.contains(':'),
        "round-tripped DATETIME should look like an ISO string, got {:?}",
        created
    );
    let updated = added
        .get("updated")
        .and_then(|v| v.as_str())
        .expect("updated column must decode as String (DATETIME → ISO string)");
    assert!(updated.contains('-') && updated.contains(':'));

    let id = added.get("id").and_then(|v| v.as_i64()).expect("id");
    let found = repo
        .find(&serde_json::Value::from(id))
        .await
        .expect("find succeeds — DATETIME decoder must not blow up")
        .expect("row exists");
    assert!(found.get("created").and_then(|v| v.as_str()).is_some());
    assert!(found.get("updated").and_then(|v| v.as_str()).is_some());
}
