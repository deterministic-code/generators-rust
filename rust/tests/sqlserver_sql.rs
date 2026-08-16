use deterministic::repositories::sqlserver::{
    SqlserverCrudRepository, SqlserverDatasource, SqlserverDatasourceOptions,
};
use deterministic::{CrudRepository, Datasource, RepositoryError, RowMap};
use serde_json::Value;
use std::sync::Arc;

fn datasource() -> Arc<SqlserverDatasource> {
    Arc::new(SqlserverDatasource::new(
        SqlserverDatasourceOptions::default(),
    ))
}

#[test]
fn build_find_sql_uses_brackets() {
    let repo = SqlserverCrudRepository::new(datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_find_sql().unwrap(),
        "SELECT * FROM [contacts] WHERE [id] = @p1"
    );
}

#[test]
fn build_add_sql_outputs_inserted_star() {
    let repo = SqlserverCrudRepository::new(datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_add_sql(&["name", "age"]).unwrap(),
        "INSERT INTO [contacts] ([name], [age]) OUTPUT INSERTED.* VALUES (@p1, @p2)"
    );
}

#[test]
fn build_update_sql_outputs_inserted_star() {
    let repo = SqlserverCrudRepository::new(datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_update_sql(&["name"]).unwrap(),
        "UPDATE [contacts] SET [name] = @p1 OUTPUT INSERTED.* WHERE [id] = @p2"
    );
}

#[test]
fn build_delete_sql_outputs_deleted_id() {
    let repo = SqlserverCrudRepository::new(datasource(), "contacts").unwrap();
    assert_eq!(
        repo.build_delete_sql().unwrap(),
        "DELETE FROM [contacts] OUTPUT DELETED.[id] WHERE [id] = @p1"
    );
}

#[tokio::test]
async fn runtime_methods_return_unimplemented() {
    let ds = datasource();
    let repo = SqlserverCrudRepository::new(Arc::clone(&ds), "contacts").unwrap();
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
async fn datasource_query_returns_unimplemented_until_tiberius_lands() {
    let ds = SqlserverDatasource::new(SqlserverDatasourceOptions::default());
    let err = ds.query("SELECT 1", &[]).await.unwrap_err();
    assert!(matches!(err, RepositoryError::Unimplemented(_)));
}
