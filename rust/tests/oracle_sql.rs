use deterministic::repositories::oracle::{
    OracleCrudRepository, OracleDatasource, OracleDatasourceOptions, OracleRepository, OracleSetup,
    OracleSetupOptions, OracleStandardRepository,
};
use deterministic::{CrudRepository, Datasource, Repository, RepositoryError, RowMap, Setup};
use serde_json::Value;
use std::sync::Arc;

fn datasource() -> Arc<OracleDatasource> {
    Arc::new(OracleDatasource::new(OracleDatasourceOptions::default()))
}

#[test]
fn crud_repository_can_build_sql_strings() {
    let repo = OracleCrudRepository::new(datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_find_sql().unwrap(),
        "SELECT * FROM \"contacts\" WHERE \"id\" = :1"
    );
    assert_eq!(
        repo.build_add_sql(&["name", "age"]).unwrap(),
        "INSERT INTO \"contacts\" (\"name\", \"age\") VALUES (:1, :2) RETURNING \"id\" INTO :3"
    );
    assert_eq!(
        repo.build_update_sql(&["name"]).unwrap(),
        "UPDATE \"contacts\" SET \"name\" = :1 WHERE \"id\" = :2"
    );
    assert_eq!(
        repo.build_delete_sql().unwrap(),
        "DELETE FROM \"contacts\" WHERE \"id\" = :1"
    );
}

#[tokio::test]
async fn datasource_methods_return_unimplemented() {
    let mut ds = OracleDatasource::new(OracleDatasourceOptions::default());
    assert!(matches!(
        ds.open().await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
    assert!(matches!(
        ds.query("SELECT 1", &[]).await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
    assert!(matches!(
        ds.close().await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
}

#[tokio::test]
async fn repository_returns_unimplemented() {
    let repo = OracleRepository::new(datasource());
    assert!(matches!(
        repo.query("SELECT 1", &[]).await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
}

#[tokio::test]
async fn crud_repository_runtime_methods_return_unimplemented() {
    let repo = OracleCrudRepository::new(datasource(), "contacts").unwrap();
    assert!(matches!(
        repo.find(&serde_json::Value::from(1)).await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
    assert!(matches!(
        repo.find_all().await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
    assert!(matches!(
        repo.find_by("name", &Value::from("a")).await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
    assert!(matches!(
        repo.add(RowMap::new()).await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
    assert!(matches!(
        repo.update(&serde_json::Value::from(1), RowMap::new())
            .await
            .unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
    assert!(matches!(
        repo.delete(&serde_json::Value::from(1)).await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
}

#[tokio::test]
async fn standard_repository_runtime_methods_return_unimplemented() {
    let repo = OracleStandardRepository::new(datasource(), "contacts").unwrap();
    assert!(matches!(
        repo.find(&serde_json::Value::from(1)).await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
    assert!(matches!(
        repo.add(RowMap::new()).await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
}

#[tokio::test]
async fn setup_returns_unimplemented() {
    let setup = OracleSetup::new(datasource(), OracleSetupOptions::default());
    assert!(matches!(
        setup.run().await.unwrap_err(),
        RepositoryError::Unimplemented(_)
    ));
}
