use async_trait::async_trait;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::row_map::RowMap;

pub type TxnOp = Box<
    dyn FnOnce(
            Arc<dyn Datasource>,
        ) -> Pin<Box<dyn Future<Output = Result<Value, RepositoryError>> + Send>>
        + Send,
>;

// why ergonomic ctor input: lets repo constructors accept either Arc<ConcreteDatasource> or Arc<dyn Datasource> without forcing `as Arc<dyn Datasource>` at every call site.
pub trait IntoDynDatasource {
    fn into_dyn_datasource(self) -> Arc<dyn Datasource>;
}

impl IntoDynDatasource for Arc<dyn Datasource> {
    fn into_dyn_datasource(self) -> Arc<dyn Datasource> {
        self
    }
}

impl<T: Datasource + 'static> IntoDynDatasource for Arc<T> {
    fn into_dyn_datasource(self) -> Arc<dyn Datasource> {
        self
    }
}

#[async_trait]
pub trait Datasource: Send + Sync {
    async fn open(&mut self) -> Result<(), RepositoryError>;
    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError>;
    async fn close(&mut self) -> Result<(), RepositoryError>;
    // why op: TxnOp (boxed closure returning BoxFuture<'static, …>): keeps the trait object-safe, lets dialect impls own commit/rollback inside the future, and avoids leaking sqlx::Transaction lifetimes through the trait surface. Mirrors typescript IDatasource.runInTransaction(async (txn) => …).
    async fn run_in_transaction(&self, op: TxnOp) -> Result<Value, RepositoryError>;
    // why: mysql INSERT does not RETURNING — repos need LAST_INSERT_ID() via the same connection/txn. Default impl runs the insert via query() and returns 0 for dialects whose INSERT shape already RETURNING-binds the id.
    async fn execute_insert_returning_id(
        &self,
        sql: &str,
        params: &[Value],
    ) -> Result<i64, RepositoryError> {
        self.query(sql, params).await?;
        Ok(0)
    }
}
