use async_trait::async_trait;
use serde_json::Value;

use crate::error::RepositoryError;
use crate::repositories::datasource::{Datasource, TxnOp};
use crate::repositories::row_map::RowMap;

#[derive(Debug, Clone, Default)]
pub struct SqlserverDatasourceOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
}

#[derive(Default)]
pub struct SqlserverDatasource {
    options: SqlserverDatasourceOptions,
    open: bool,
}

impl SqlserverDatasource {
    pub fn new(options: SqlserverDatasourceOptions) -> Self {
        Self {
            options,
            open: false,
        }
    }

    pub fn options(&self) -> &SqlserverDatasourceOptions {
        &self.options
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
}

#[async_trait]
impl Datasource for SqlserverDatasource {
    async fn open(&mut self) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "sqlserver backend not available in Rust ecosystem (tiberius integration pending)",
        ))
    }

    async fn query(&self, _sql: &str, _params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "sqlserver backend not available in Rust ecosystem (tiberius integration pending)",
        ))
    }

    async fn close(&mut self) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "sqlserver backend not available in Rust ecosystem (tiberius integration pending)",
        ))
    }

    async fn run_in_transaction(&self, _op: TxnOp) -> Result<Value, RepositoryError> {
        Err(RepositoryError::TransactionsNotSupported(
            "sqlserver driver not yet wired (tiberius integration pending)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_fails_loudly_with_unimplemented_error() {
        let mut ds = SqlserverDatasource::default();
        let err = ds.open().await.unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("sqlserver backend not available"),
            "expected clear sqlserver message, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn query_fails_without_going_through_open() {
        let ds = SqlserverDatasource::default();
        let err = ds.query("SELECT 1", &[]).await.unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("sqlserver backend not available"),
            "expected clear sqlserver message, got: {}",
            msg
        );
    }
}
