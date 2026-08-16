use deterministic::repositories::mysql::{MysqlDatasource, MysqlDatasourceOptions};
use deterministic::RepositoryError;
use std::sync::Arc;

fn unopened_datasource() -> Arc<MysqlDatasource> {
    Arc::new(MysqlDatasource::new(MysqlDatasourceOptions {
        url: "mysql://test:test@127.0.0.1/none".to_string(),
        max_connections: 1,
    }))
}

#[tokio::test]
async fn mysql_datasource_exposes_execute_unprepared_returning_not_open_when_closed() {
    let ds = unopened_datasource();
    let res = ds
        .execute_unprepared("DROP PROCEDURE IF EXISTS p_test")
        .await;
    match res {
        Err(RepositoryError::NotOpen) => {}
        other => panic!("expected RepositoryError::NotOpen on closed pool, got {other:?}"),
    }
}

#[tokio::test]
async fn mysql_datasource_execute_unprepared_routes_through_raw_sql_not_prepared() {
    let ds = unopened_datasource();
    let res = ds
        .execute_unprepared("CREATE PROCEDURE p_test() BEGIN SELECT 1; END")
        .await;
    assert!(matches!(res, Err(RepositoryError::NotOpen)));
}
