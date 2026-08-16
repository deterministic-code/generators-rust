use async_trait::async_trait;
use serde_json::Value;

use crate::error::RepositoryError;
use crate::repositories::datasource::{Datasource, TxnOp};
use crate::repositories::row_map::RowMap;

#[derive(Debug, Clone, Default)]
pub struct OracleDatasourceOptions {
    pub connect_string: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
}

#[derive(Default)]
pub struct OracleDatasource {
    options: OracleDatasourceOptions,
}

impl OracleDatasource {
    pub fn new(options: OracleDatasourceOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &OracleDatasourceOptions {
        &self.options
    }
}

#[async_trait]
impl Datasource for OracleDatasource {
    async fn open(&mut self) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "oracle backend not available in Rust ecosystem",
        ))
    }

    async fn query(&self, _sql: &str, _params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "oracle backend not available in Rust ecosystem",
        ))
    }

    async fn close(&mut self) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "oracle backend not available in Rust ecosystem",
        ))
    }

    async fn run_in_transaction(&self, _op: TxnOp) -> Result<Value, RepositoryError> {
        Err(RepositoryError::TransactionsNotSupported(
            "oracle backend not available in Rust ecosystem".into(),
        ))
    }
}
