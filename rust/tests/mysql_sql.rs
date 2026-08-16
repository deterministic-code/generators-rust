use deterministic::mappings::{EntityFieldMap, FieldMappingTranslator};
use deterministic::repositories::mysql::{
    MysqlCrudRepository, MysqlDatasource, MysqlDatasourceOptions,
};
use deterministic::{Datasource, RepositoryError};
use std::sync::Arc;

fn unopened_datasource() -> Arc<MysqlDatasource> {
    Arc::new(MysqlDatasource::new(MysqlDatasourceOptions {
        url: "mysql://test:test@127.0.0.1/none".to_string(),
        max_connections: 1,
    }))
}

#[test]
fn build_find_sql_uses_question_mark_and_backticks() {
    let repo = MysqlCrudRepository::new(unopened_datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_find_sql().unwrap(),
        "SELECT * FROM `contacts` WHERE `id` = ?"
    );
}

#[test]
fn build_add_sql_no_returning() {
    let repo = MysqlCrudRepository::new(unopened_datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_add_sql(&["name", "age"]).unwrap(),
        "INSERT INTO `contacts` (`name`, `age`) VALUES (?, ?)"
    );
}

#[test]
fn build_update_sql_uses_question_marks() {
    let repo = MysqlCrudRepository::new(unopened_datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_update_sql(&["name"]).unwrap(),
        "UPDATE `contacts` SET `name` = ? WHERE `id` = ?"
    );
}

#[test]
fn with_field_mapping_translator_chains_on_mysql_crud_repository() {
    // why: emit-rust-cargo-project.mjs emits
    // MysqlCrudRepository::new(...).with_field_mapping_translator(translator).with_middlewares(...)
    // for any entity with fieldMappings. Without this method the generated
    // main.rs fails cargo build with E0599 "no method named
    // with_field_mapping_translator found for struct MysqlCrudRepository".
    let mut map = EntityFieldMap::new();
    map.insert("key".to_string(), "MsgID".to_string());
    let translator = Arc::new(FieldMappingTranslator::new(Some(&map)).unwrap());
    let repo = MysqlCrudRepository::new(unopened_datasource(), "contacts")
        .unwrap()
        .with_field_mapping_translator(translator);
    assert_eq!(repo.primary_key(), "id");
}

#[tokio::test]
async fn datasource_query_without_open_returns_not_open() {
    let ds = MysqlDatasource::new(MysqlDatasourceOptions {
        url: "mysql://x/y".to_string(),
        max_connections: 1,
    });
    let err = ds.query("SELECT 1", &[]).await.unwrap_err();
    assert!(matches!(err, RepositoryError::NotOpen));
}
