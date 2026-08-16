mod common;

use common::{migrate_provider_for, repo_root};
use deterministic::repositories::postgres::{PostgresDatasource, PostgresDatasourceOptions};
use deterministic::Datasource;
use serde_json::{json, Value};
use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres;

fn docker_disabled() -> bool {
    std::env::var("SKIP_DOCKER_TESTS").is_ok()
}

fn social_messaging_deterministic() -> std::path::PathBuf {
    repo_root()
        .join("samples")
        .join("social-messaging-backend")
        .join("deterministic")
}

async fn open_datasource(url: &str) -> Arc<PostgresDatasource> {
    let mut ds = PostgresDatasource::new(PostgresDatasourceOptions {
        url: url.to_string(),
        max_connections: 2,
    });
    ds.open().await.expect("open postgres");
    Arc::new(ds)
}

fn container_url(host: &str, port: u16) -> String {
    format!("postgres://postgres:postgres@{}:{}/postgres", host, port)
}

/// The emitted `migrate-up` binary must apply BOTH `0001_initial` and `0002_stored_procedures` from the real social-messaging sample, and the plpgsql/sql functions in 0002 must be callable afterward — proving procedures ride the migration chain rather than a sidecar. Exercises `create_member` (RETURNS UUID) and the datasource-derived `find_member_by_handle` by-field lookup.
#[tokio::test]
async fn migrate_up_applies_0002_and_procedures_are_callable() {
    if docker_disabled() {
        return;
    }
    // pin postgres 16 to match the real verify pipeline: the emitted 0001_initial uses core gen_random_uuid(), built in from pg13+, absent on the testcontainers default tag.
    let container: ContainerAsync<Postgres> = Postgres::default()
        .with_tag("16-alpine")
        .start()
        .await
        .expect("start postgres testcontainer (is docker running?)");
    let host = container
        .get_host()
        .await
        .expect("container host")
        .to_string();
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");
    let url = container_url(&host, port);

    let sample = social_messaging_deterministic();
    let _sql_tree = migrate_provider_for(&sample, "postgres", &url);

    let ds = open_datasource(&url).await;

    // id_type: uuid → the pk IS the uuid, so create_member takes no separate uuid param.
    let created = ds
        .query(
            "SELECT create_member($1, $2, $3, now(), now()) AS id",
            &[
                json!("member-fn-test"),
                json!("Fn Test"),
                json!("integration bio"),
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
        .query(
            "SELECT * FROM find_member_by_handle($1)",
            &[json!("member-fn-test")],
        )
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
