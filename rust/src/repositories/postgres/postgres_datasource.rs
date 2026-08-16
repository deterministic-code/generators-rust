use async_trait::async_trait;
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::RepositoryError;
use crate::repositories::datasource::{Datasource, TxnOp};
use crate::repositories::postgres::column_decoder::{
    PostgresColumnDecoder, PostgresDecoderRegistry,
};
use crate::repositories::row_map::RowMap;

#[derive(Debug, Clone)]
pub struct PostgresDatasourceOptions {
    pub url: String,
    pub max_connections: u32,
}

pub struct PostgresDatasource {
    options: PostgresDatasourceOptions,
    pool: Option<PgPool>,
    decoders: PostgresDecoderRegistry,
}

impl PostgresDatasource {
    pub fn new(options: PostgresDatasourceOptions) -> Self {
        Self {
            options,
            pool: None,
            decoders: PostgresDecoderRegistry::new(),
        }
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            options: PostgresDatasourceOptions {
                url: String::new(),
                max_connections: 1,
            },
            pool: Some(pool),
            decoders: PostgresDecoderRegistry::new(),
        }
    }

    pub fn with_decoder(mut self, decoder: Arc<dyn PostgresColumnDecoder>) -> Self {
        self.decoders = self.decoders.with_decoder(decoder);
        self
    }

    pub fn pool(&self) -> Result<&PgPool, RepositoryError> {
        self.pool.as_ref().ok_or(RepositoryError::NotOpen)
    }
}

#[async_trait]
impl Datasource for PostgresDatasource {
    async fn open(&mut self) -> Result<(), RepositoryError> {
        if self.pool.is_some() {
            return Ok(());
        }
        let pool = PgPoolOptions::new()
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

    async fn close(&mut self) -> Result<(), RepositoryError> {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
        Ok(())
    }

    async fn run_in_transaction(&self, op: TxnOp) -> Result<Value, RepositoryError> {
        let pool = self.pool()?;
        let txn = pool.begin().await.map_err(RepositoryError::Sqlx)?;
        let cell: Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Postgres>>>> =
            Arc::new(Mutex::new(Some(txn)));
        let txn_ds: Arc<dyn Datasource> = Arc::new(PostgresTxnDatasource {
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

pub struct PostgresTxnDatasource {
    cell: Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Postgres>>>>,
    decoders: PostgresDecoderRegistry,
}

#[async_trait]
impl Datasource for PostgresTxnDatasource {
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

    async fn run_in_transaction(&self, _op: TxnOp) -> Result<Value, RepositoryError> {
        Err(RepositoryError::TransactionsNotSupported(
            "nested transactions are not supported on postgres".into(),
        ))
    }
}

fn bind_value<'a>(
    q: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    v: &'a Value,
) -> sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments> {
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
