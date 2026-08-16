use deterministic::error::RepositoryError;
use deterministic::mappings::{
    EntityFieldMap, FieldMappingTranslator, PostgresDateTimeConverter, TypeFieldConverter,
};
use deterministic::repositories::postgres::{
    PostgresColumnDecoder, PostgresCrudRepository, PostgresDatasource, PostgresDatasourceOptions,
    PostgresStandardRepository,
};
use deterministic::{CrudRepository, Datasource, RowMap};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::Row as _;
use std::sync::Arc;
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

async fn drop_and_create(ds: &Arc<PostgresDatasource>, table: &str, schema: &str) {
    ds.query(&format!("DROP TABLE IF EXISTS {}", table), &[])
        .await
        .expect("drop table");
    ds.query(schema, &[]).await.expect("create table");
}

#[tokio::test]
async fn postgres_live_text_column_routes_through_text_decoder_override() {
    // why: locks the open extension point for postgres — a user-registered PostgresColumnDecoder must run BEFORE the builtin PostgresTextColumnDecoder so app code can override how text cells render. Asserts the chain ordering for TEXT and VARCHAR with a fall-through for INT through PostgresNumericColumnDecoder.
    if docker_disabled() {
        return;
    }
    struct ShoutingTextDecoder;
    impl PostgresColumnDecoder for ShoutingTextDecoder {
        fn decode(
            &self,
            row: &PgRow,
            idx: usize,
            type_name: &str,
        ) -> Result<Option<Value>, RepositoryError> {
            match type_name {
                "TEXT" | "VARCHAR" | "BPCHAR" | "CHAR" | "NAME" => {
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
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    let mut ds = PostgresDatasource::new(PostgresDatasourceOptions {
        url,
        max_connections: 2,
    })
    .with_decoder(Arc::new(ShoutingTextDecoder));
    ds.open().await.expect("open postgres with custom decoder");
    let ds = Arc::new(ds);
    drop_and_create(
        &ds,
        "text_decoder_test",
        "CREATE TABLE text_decoder_test (id BIGSERIAL PRIMARY KEY, label VARCHAR(64) NOT NULL, body TEXT NOT NULL, count INT NOT NULL)",
    )
    .await;

    ds.query(
        "INSERT INTO text_decoder_test (label, body, count) VALUES ($1, $2, $3)",
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
        "VARCHAR must route through the custom decoder, not the builtin Text"
    );
    assert_eq!(
        rows[0].get("body").and_then(|v| v.as_str()),
        Some("ALSO-QUIET"),
        "TEXT must route through the custom decoder too"
    );
    assert_eq!(
        rows[0].get("count").and_then(|v| v.as_i64()),
        Some(42),
        "INT must fall through to PostgresNumericColumnDecoder, not the text override"
    );
}

#[tokio::test]
async fn postgres_live_crud_roundtrip() {
    if docker_disabled() {
        eprintln!("SKIP_DOCKER_TESTS set — skipping postgres live test");
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "test_widget",
        "CREATE TABLE test_widget (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL)",
    )
    .await;

    let repo = PostgresCrudRepository::new(Arc::clone(&ds), "test_widget").unwrap();

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

    let all = repo.find_all().await.expect("find_all");
    assert!(!all.is_empty());

    let deleted = repo
        .delete(&serde_json::Value::from(id))
        .await
        .expect("delete");
    assert!(deleted);
}

#[tokio::test]
async fn postgres_live_standard_repository_populates_audit_columns() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "test_audited",
        "CREATE TABLE test_audited (id BIGSERIAL PRIMARY KEY, uuid UUID NOT NULL, name TEXT NOT NULL, created TIMESTAMPTZ NOT NULL DEFAULT (NOW() AT TIME ZONE 'UTC'), updated TIMESTAMPTZ NOT NULL DEFAULT (NOW() AT TIME ZONE 'UTC'))",
    )
    .await;

    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "test_audited").unwrap();

    let mut row = RowMap::new();
    row.insert("name".to_string(), Value::from("beta"));
    let created = repo.add(row).await.expect("insert");

    assert!(created.get("uuid").and_then(|v| v.as_str()).is_some());
    assert!(created.get("created").and_then(|v| v.as_str()).is_some());
    assert!(created.get("updated").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn postgres_live_find_by_returns_matching_rows() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "test_widget",
        "CREATE TABLE test_widget (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL)",
    )
    .await;
    let repo = PostgresCrudRepository::new(Arc::clone(&ds), "test_widget").unwrap();

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

// Regression for e2e_rust_postgres legacy_contacts 500: without `.with_column_datasource_types` + `.with_converters` registering PostgresDateTimeConverter, sqlx binds JSON strings as TEXT and the INSERT fails because postgres won't implicitly cast TEXT→TIMESTAMPTZ.
#[tokio::test]
async fn postgres_live_crud_repo_binds_datetime_for_legacy_contact_shape() {
    if docker_disabled() {
        eprintln!("SKIP_DOCKER_TESTS set — skipping postgres live test");
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "\"OldContactsTbl\"",
        "CREATE TABLE \"OldContactsTbl\" (\
            \"CntID\" VARCHAR(64) PRIMARY KEY, \
            \"FirstNm\" VARCHAR(128) NOT NULL, \
            \"ImpDate\" TIMESTAMPTZ\
        )",
    )
    .await;

    let mut field_map = EntityFieldMap::new();
    field_map.insert("key".to_string(), "CntID".to_string());
    field_map.insert("first_name".to_string(), "FirstNm".to_string());
    field_map.insert("imported_at".to_string(), "ImpDate".to_string());
    let translator = Arc::new(FieldMappingTranslator::new(Some(&field_map)).unwrap());
    let mut col_types: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    col_types.insert("imported_at".to_string(), "datetime".to_string());

    let repo = PostgresCrudRepository::new(Arc::clone(&ds), "OldContactsTbl")
        .unwrap()
        .with_primary_key("key")
        .unwrap()
        .with_field_mapping_translator(translator)
        .with_column_datasource_types(col_types)
        .with_converters(vec![
            Arc::new(PostgresDateTimeConverter) as Arc<dyn TypeFieldConverter>
        ]);

    let mut row = RowMap::new();
    row.insert("key".to_string(), Value::from("sample-0"));
    row.insert("first_name".to_string(), Value::from("Alice"));
    row.insert(
        "imported_at".to_string(),
        Value::from("2024-01-01T00:00:00.000Z"),
    );
    let added = repo
        .add(row)
        .await
        .expect("add must succeed — datetime bind cast injected by converter");
    assert_eq!(added.get("key").and_then(|v| v.as_str()), Some("sample-0"));
    assert!(added.get("CntID").is_none(), "physical name must not leak");
}

// Mirrors *_crud_repo_binds_datetime_for_legacy_contact_shape on PostgresStandardRepository: with_column_datasource_types + with_converters must extend the audit defaults so user datetime columns get ::timestamptz, else POST /api/messages 500s.
#[tokio::test]
async fn postgres_live_standard_repo_binds_datetime_for_user_received_at_column() {
    if docker_disabled() {
        eprintln!("SKIP_DOCKER_TESTS set — skipping postgres live test");
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    // why no DB-side defaults: bare testcontainers postgres lacks pgcrypto/gen_random_uuid, and StandardRepository.add() already fills uuid/created/updated client-side.
    drop_and_create(
        &ds,
        "\"messages\"",
        "CREATE TABLE \"messages\" (\
            \"id\" SERIAL PRIMARY KEY, \
            \"uuid\" UUID NOT NULL UNIQUE, \
            \"folder_id\" INTEGER NOT NULL, \
            \"subject\" VARCHAR(512) NOT NULL, \
            \"received_at\" TIMESTAMPTZ, \
            \"created\" TIMESTAMPTZ NOT NULL, \
            \"updated\" TIMESTAMPTZ NOT NULL\
        )",
    )
    .await;

    let mut col_types: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    col_types.insert("received_at".to_string(), "datetime".to_string());
    let repo = PostgresStandardRepository::new(Arc::clone(&ds), "messages")
        .unwrap()
        .with_column_datasource_types(col_types)
        .with_converters(vec![
            Arc::new(PostgresDateTimeConverter) as Arc<dyn TypeFieldConverter>
        ]);

    let mut row = RowMap::new();
    row.insert("folder_id".to_string(), Value::from(1_i64));
    row.insert("subject".to_string(), Value::from("sample-0"));
    row.insert(
        "received_at".to_string(),
        Value::from("2024-01-01T00:00:00.000Z"),
    );
    let added = repo
        .add(row)
        .await
        .expect("add must succeed — ::timestamptz cast injected for user datetime column AND audit defaults preserved");
    assert_eq!(
        added.get("subject").and_then(|v| v.as_str()),
        Some("sample-0")
    );
    assert!(
        added.get("received_at").is_some(),
        "received_at must round-trip"
    );
    assert!(added.get("uuid").is_some(), "audit uuid must populate");
    assert!(
        added.get("created").is_some(),
        "audit created must populate"
    );
    assert!(
        added.get("updated").is_some(),
        "audit updated must populate"
    );
}

// Negative companion: same shape WITHOUT .with_converters() must fail with the production error, locking in the dependency on the converter chain.
#[tokio::test]
async fn postgres_live_crud_repo_without_converter_fails_with_text_to_timestamptz_error() {
    if docker_disabled() {
        return;
    }
    let container = start_container().await;
    let ds = open_datasource(&container).await;
    drop_and_create(
        &ds,
        "\"OldContactsTbl2\"",
        "CREATE TABLE \"OldContactsTbl2\" (\
            \"CntID\" VARCHAR(64) PRIMARY KEY, \
            \"ImpDate\" TIMESTAMPTZ\
        )",
    )
    .await;
    let repo = PostgresCrudRepository::new(Arc::clone(&ds), "OldContactsTbl2")
        .unwrap()
        .with_primary_key("CntID")
        .unwrap();
    let mut row = RowMap::new();
    row.insert("CntID".to_string(), Value::from("sample"));
    row.insert(
        "ImpDate".to_string(),
        Value::from("2024-01-01T00:00:00.000Z"),
    );
    let err = repo
        .add(row)
        .await
        .expect_err("must reject text→timestamptz bind without converter");
    let msg = format!("{}", err);
    assert!(
        msg.contains("timestamp with time zone")
            || msg.contains("text")
            || msg.to_lowercase().contains("type"),
        "expected postgres type-mismatch error, got: {msg}",
    );
}
