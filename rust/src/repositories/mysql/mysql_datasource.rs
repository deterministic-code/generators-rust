use async_trait::async_trait;
use serde_json::Value;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::RepositoryError;
use crate::repositories::datasource::{Datasource, TxnOp};
use crate::repositories::mysql::column_decoder::{MysqlColumnDecoder, MysqlDecoderRegistry};
use crate::repositories::row_map::RowMap;

#[derive(Debug, Clone)]
pub struct MysqlDatasourceOptions {
    pub url: String,
    pub max_connections: u32,
}

pub struct MysqlDatasource {
    options: MysqlDatasourceOptions,
    pool: Option<MySqlPool>,
    decoders: MysqlDecoderRegistry,
}

impl MysqlDatasource {
    pub fn new(options: MysqlDatasourceOptions) -> Self {
        Self {
            options,
            pool: None,
            decoders: MysqlDecoderRegistry::new(),
        }
    }

    pub fn from_pool(pool: MySqlPool) -> Self {
        Self {
            options: MysqlDatasourceOptions {
                url: String::new(),
                max_connections: 1,
            },
            pool: Some(pool),
            decoders: MysqlDecoderRegistry::new(),
        }
    }

    pub fn with_decoder(mut self, decoder: Arc<dyn MysqlColumnDecoder>) -> Self {
        self.decoders = self.decoders.with_decoder(decoder);
        self
    }

    pub fn pool(&self) -> Result<&MySqlPool, RepositoryError> {
        self.pool.as_ref().ok_or(RepositoryError::NotOpen)
    }

    pub async fn execute_insert_returning_id(
        &self,
        sql: &str,
        params: &[Value],
    ) -> Result<i64, RepositoryError> {
        let pool = self.pool()?;
        let mut q = sqlx::query(sql);
        for p in params {
            q = bind_value(q, p);
        }
        let result = q.execute(pool).await.map_err(RepositoryError::Sqlx)?;
        Ok(result.last_insert_id() as i64)
    }

    pub async fn execute_unprepared(&self, sql: &str) -> Result<(), RepositoryError> {
        let pool = self.pool()?;
        sqlx::raw_sql(sql)
            .execute(pool)
            .await
            .map_err(RepositoryError::Sqlx)?;
        Ok(())
    }
}

#[async_trait]
impl Datasource for MysqlDatasource {
    async fn open(&mut self) -> Result<(), RepositoryError> {
        if self.pool.is_some() {
            return Ok(());
        }
        let pool = MySqlPoolOptions::new()
            .max_connections(self.options.max_connections)
            .connect(&self.options.url)
            .await
            .map_err(RepositoryError::Sqlx)?;
        self.pool = Some(pool);
        Ok(())
    }

    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        let pool = self.pool()?;
        let mut q = sqlx::query(sql);
        for p in params {
            q = bind_value(q, p);
        }
        let rows = q.fetch_all(pool).await.map_err(RepositoryError::Sqlx)?;
        self.decoders.decode_rows(rows)
    }

    async fn execute_insert_returning_id(
        &self,
        sql: &str,
        params: &[Value],
    ) -> Result<i64, RepositoryError> {
        MysqlDatasource::execute_insert_returning_id(self, sql, params).await
    }

    async fn close(&mut self) -> Result<(), RepositoryError> {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
        Ok(())
    }

    async fn run_in_transaction(&self, op: TxnOp) -> Result<Value, RepositoryError> {
        let pool = self.pool()?;
        let txn = pool.begin().await.map_err(RepositoryError::Sqlx)?;
        let cell: Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::MySql>>>> =
            Arc::new(Mutex::new(Some(txn)));
        let txn_ds: Arc<dyn Datasource> = Arc::new(MysqlTxnDatasource {
            cell: cell.clone(),
            decoders: self.decoders.clone(),
        });
        let result = op(txn_ds).await;
        let mut guard = cell.lock().await;
        let txn = guard
            .take()
            .ok_or_else(|| RepositoryError::Other("transaction missing at completion".into()))?;
        drop(guard);
        match result {
            Ok(v) => {
                txn.commit().await.map_err(RepositoryError::Sqlx)?;
                Ok(v)
            }
            Err(e) => {
                let _ = txn.rollback().await;
                Err(e)
            }
        }
    }
}

pub struct MysqlTxnDatasource {
    cell: Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::MySql>>>>,
    decoders: MysqlDecoderRegistry,
}

#[async_trait]
impl Datasource for MysqlTxnDatasource {
    async fn open(&mut self) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn close(&mut self) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        let mut guard = self.cell.lock().await;
        let txn = guard
            .as_mut()
            .ok_or_else(|| RepositoryError::Other("transaction already completed".into()))?;
        let mut q = sqlx::query(sql);
        for p in params {
            q = bind_value(q, p);
        }
        let rows = q
            .fetch_all(&mut **txn)
            .await
            .map_err(RepositoryError::Sqlx)?;
        self.decoders.decode_rows(rows)
    }

    async fn execute_insert_returning_id(
        &self,
        sql: &str,
        params: &[Value],
    ) -> Result<i64, RepositoryError> {
        let mut guard = self.cell.lock().await;
        let txn = guard
            .as_mut()
            .ok_or_else(|| RepositoryError::Other("transaction already completed".into()))?;
        let mut q = sqlx::query(sql);
        for p in params {
            q = bind_value(q, p);
        }
        let result = q.execute(&mut **txn).await.map_err(RepositoryError::Sqlx)?;
        Ok(result.last_insert_id() as i64)
    }

    async fn run_in_transaction(&self, _op: TxnOp) -> Result<Value, RepositoryError> {
        Err(RepositoryError::TransactionsNotSupported(
            "nested transactions are not supported on mysql".into(),
        ))
    }
}

fn bind_value<'a>(
    q: sqlx::query::Query<'a, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    v: &'a Value,
) -> sqlx::query::Query<'a, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    match v {
        Value::Null => q.bind(Option::<String>::None),
        Value::Bool(b) => q.bind(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                q.bind(i)
            } else if let Some(f) = n.as_f64() {
                q.bind(f)
            } else {
                q.bind(n.to_string())
            }
        }
        Value::String(s) => q.bind(s.clone()),
        other => q.bind(other.to_string()),
    }
}
