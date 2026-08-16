use deterministic::mappings::{EntityFieldMap, FieldMappingTranslator};
use deterministic::repositories::postgres::{
    PostgresCrudRepository, PostgresDatasource, PostgresDatasourceOptions,
};
use deterministic::{Datasource, RepositoryError};
use std::sync::Arc;

fn unopened_datasource() -> Arc<PostgresDatasource> {
    Arc::new(PostgresDatasource::new(PostgresDatasourceOptions {
        url: "postgres://test:test@127.0.0.1/none".to_string(),
        max_connections: 1,
    }))
}

#[test]
fn build_find_sql_uses_dollar_one() {
    let repo = PostgresCrudRepository::new(unopened_datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_find_sql().unwrap(),
        "SELECT * FROM \"contacts\" WHERE \"id\" = $1"
    );
}

#[test]
fn build_add_sql_returns_star() {
    let repo = PostgresCrudRepository::new(unopened_datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_add_sql(&["name", "age"]).unwrap(),
        "INSERT INTO \"contacts\" (\"name\", \"age\") VALUES ($1, $2) RETURNING *"
    );
}

#[test]
fn build_update_sql_indexes_id_last() {
    let repo = PostgresCrudRepository::new(unopened_datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_update_sql(&["name"]).unwrap(),
        "UPDATE \"contacts\" SET \"name\" = $1 WHERE \"id\" = $2 RETURNING *"
    );
}

#[test]
fn build_delete_sql_returns_id() {
    let repo = PostgresCrudRepository::new(unopened_datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_delete_sql().unwrap(),
        "DELETE FROM \"contacts\" WHERE \"id\" = $1 RETURNING \"id\""
    );
}

#[test]
fn with_field_mapping_translator_chains_on_postgres_crud_repository() {
    // why: emit-rust-cargo-project.mjs emits
    // PostgresCrudRepository::new(...).with_field_mapping_translator(translator).with_middlewares(...)
    // for any entity with fieldMappings. Without this method the generated
    // main.rs fails cargo build with E0599 "no method named
    // with_field_mapping_translator found for struct PostgresCrudRepository".
    let mut map = EntityFieldMap::new();
    map.insert("key".to_string(), "MsgID".to_string());
    let translator = Arc::new(FieldMappingTranslator::new(Some(&map)).unwrap());
    let repo = PostgresCrudRepository::new(unopened_datasource(), "contacts")
        .unwrap()
        .with_field_mapping_translator(translator);
    assert_eq!(repo.primary_key(), "id");
}

#[tokio::test]
async fn datasource_query_without_open_returns_not_open() {
    let ds = PostgresDatasource::new(PostgresDatasourceOptions {
        url: "postgres://x/y".to_string(),
        max_connections: 1,
    });
    let err = ds.query("SELECT 1", &[]).await.unwrap_err();
    assert!(matches!(err, RepositoryError::NotOpen));
}
