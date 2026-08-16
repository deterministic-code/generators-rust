mod common;

use common::{migrate_provider_for, repo_root};
use deterministic::repositories::mysql::{MysqlDatasource, MysqlDatasourceOptions};
use deterministic::Datasource;
use serde_json::{json, Value};
use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::mysql::Mysql;

fn docker_disabled() -> bool {
    std::env::var("SKIP_DOCKER_TESTS").is_ok()
}

fn social_messaging_deterministic() -> std::path::PathBuf {
    repo_root()
        .join("samples")
        .join("social-messaging-backend")
        .join("deterministic")
}

async fn open_datasource(url: &str) -> Arc<MysqlDatasource> {
    let mut ds = MysqlDatasource::new(MysqlDatasourceOptions {
        url: url.to_string(),
        max_connections: 2,
    });
    ds.open().await.expect("open mysql");
    Arc::new(ds)
}

/// The emitted `migrate-up` binary must apply BOTH `0001_initial` and the GO-separated `0002_stored_procedures` from the real social-messaging sample on mysql, and the procedures must be callable via `CALL`. For `id_type: uuid` the create procedure generates the pk in-proc (mysql has no RETURNING and there is no system `uuid` column), which this test exercises through `create_member` + `find_member_by_handle`.
#[tokio::test]
async fn migrate_up_applies_0002_and_procedures_are_callable() {
    if docker_disabled() {
        return;
    }
    // pin mysql 8 to match the real verify pipeline: 0001_initial uses a `DEFAULT (UUID())` expression default (mysql 8.0.13+).
    let container: ContainerAsync<Mysql> = Mysql::default()
        .with_tag("8")
        .start()
        .await
        .expect("start mysql testcontainer (is docker running?)");
    let host = container
        .get_host()
        .await
        .expect("container host")
        .to_string();
    let port = container
        .get_host_port_ipv4(3306)
        .await
        .expect("container port");
    let url = format!("mysql://root@{}:{}/test", host, port);

    let sample = social_messaging_deterministic();
    let _sql_tree = migrate_provider_for(&sample, "mysql", &url);

    let ds = open_datasource(&url).await;

    let created = ds
        .query(
            "CALL create_member(?, ?, ?, ?, ?)",
            &[
                json!("member-fn-test"),
                json!("Fn Test"),
                json!("integration bio"),
                json!("2024-01-01T00:00:00Z"),
                json!("2024-01-01T00:00:00Z"),
            ],
        )
        .await
        .expect("call create_member");
    assert_eq!(created.len(), 1, "create_member returns exactly one id row");
    assert!(
        matches!(created[0].get("id"), Some(Value::String(_))),
        "create_member returns a uuid id, got {:?}",
        created[0].get("id")
    );

    let rows = ds
        .query("CALL find_member_by_handle(?)", &[json!("member-fn-test")])
        .await
        .expect("call find_member_by_handle");
    assert_eq!(
        rows.len(),
        1,
        "find_member_by_handle returns the member inserted through create_member"
    );
    assert_eq!(
        rows[0].get("handle").and_then(Value::as_str),
        Some("member-fn-test")
    );
    assert_eq!(
        rows[0].get("display_name").and_then(Value::as_str),
        Some("Fn Test")
    );
}
