use async_trait::async_trait;
use serde_json::Value;
use sqlx::pool::PoolConnection;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::Sqlite;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::error::RepositoryError;
use crate::repositories::datasource::{Datasource, TxnOp};
use crate::repositories::row_map::RowMap;
use crate::repositories::sqlite::column_decoder::{SqliteColumnDecoder, SqliteDecoderRegistry};

#[derive(Debug, Clone)]
pub struct SqliteDatasourceOptions {
    pub url: String,
    pub max_connections: u32,
}

impl SqliteDatasourceOptions {
    pub fn in_memory() -> Self {
        Self {
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
        }
    }
}

pub struct SqliteDatasource {
    options: SqliteDatasourceOptions,
    pool: Option<SqlitePool>,
    decoders: SqliteDecoderRegistry,
}

impl SqliteDatasource {
    pub fn new(options: SqliteDatasourceOptions) -> Self {
        Self {
            options,
            pool: None,
            decoders: SqliteDecoderRegistry::new(),
        }
    }

    pub fn from_pool(pool: SqlitePool) -> Self {
        Self {
            options: SqliteDatasourceOptions::in_memory(),
            pool: Some(pool),
            decoders: SqliteDecoderRegistry::new(),
        }
    }

    pub fn with_decoder(mut self, decoder: Arc<dyn SqliteColumnDecoder>) -> Self {
        self.decoders = self.decoders.with_decoder(decoder);
        self
    }

    pub fn pool(&self) -> Result<&SqlitePool, RepositoryError> {
        self.pool.as_ref().ok_or(RepositoryError::NotOpen)
    }
}

#[async_trait]
impl Datasource for SqliteDatasource {
    async fn open(&mut self) -> Result<(), RepositoryError> {
        if self.pool.is_some() {
            return Ok(());
        }
        let mut connect_options = SqliteConnectOptions::from_str(&self.options.url)
            .map_err(RepositoryError::Sqlx)?
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5));
        // why WAL only for file-backed sqlite: `:memory:` is private per-connection and the rollback journal is the only mode that makes sense; WAL pragmas are silently ignored there, but skipping the call keeps the intent explicit.
        if !is_in_memory_url(&self.options.url) {
            connect_options = connect_options
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal);
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(self.options.max_connections)
            .connect_with(connect_options)
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
        let mut conn = pool.acquire().await.map_err(RepositoryError::Sqlx)?;
        // why BEGIN IMMEDIATE: sqlx's `pool.begin()` issues BEGIN DEFERRED, which acquires a read snapshot lazily and then upgrades to a writer lock. In WAL mode, that upgrade fails immediately with SQLITE_BUSY_SNAPSHOT (extended code 517) when another connection has committed since the snapshot was taken — and busy_timeout does NOT retry on 517. BEGIN IMMEDIATE takes the writer lock upfront so concurrent writers wait on busy_timeout instead of racing into a snapshot conflict.
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(RepositoryError::Sqlx)?;
        let cell: Arc<Mutex<Option<PoolConnection<Sqlite>>>> = Arc::new(Mutex::new(Some(conn)));
        let txn_ds: Arc<dyn Datasource> = Arc::new(SqliteTxnDatasource {
            cell: cell.clone(),
            decoders: self.decoders.clone(),
        });
        let result = op(txn_ds).await;
        // why take() under lock: SqliteTxnDatasource holds an Arc clone; reclaiming the connection requires extracting it from the shared cell so we can commit/rollback by value.
        let mut guard = cell.lock().await;
        let mut conn = guard
            .take()
            .ok_or_else(|| RepositoryError::Other("transaction missing at completion".into()))?;
        drop(guard);
        match result {
            Ok(v) => {
                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(RepositoryError::Sqlx)?;
                Ok(v)
            }
            Err(e) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                Err(e)
            }
        }
    }
}

pub struct SqliteTxnDatasource {
    cell: Arc<Mutex<Option<PoolConnection<Sqlite>>>>,
    decoders: SqliteDecoderRegistry,
}

#[async_trait]
impl Datasource for SqliteTxnDatasource {
    async fn open(&mut self) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn close(&mut self) -> Result<(), RepositoryError> {
        Ok(())
    }

    async fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<RowMap>, RepositoryError> {
        let mut guard = self.cell.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| RepositoryError::Other("transaction already completed".into()))?;
        let mut q = sqlx::query(sql);
        for p in params {
            q = bind_value(q, p);
        }
        let rows = q
            .fetch_all(&mut **conn)
            .await
            .map_err(RepositoryError::Sqlx)?;
        self.decoders.decode_rows(rows)
    }

    async fn run_in_transaction(&self, _op: TxnOp) -> Result<Value, RepositoryError> {
        Err(RepositoryError::TransactionsNotSupported(
            "nested transactions are not supported on sqlite".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn open_in_memory() -> SqliteDatasource {
        let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
        ds.open().await.unwrap();
        ds.query(
            "CREATE TABLE thing (id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT)",
            &[],
        )
        .await
        .unwrap();
        ds
    }

    #[tokio::test]
    async fn run_in_transaction_commits_on_ok() {
        let ds = open_in_memory().await;
        let result = ds
            .run_in_transaction(Box::new(|txn_ds| {
                Box::pin(async move {
                    txn_ds
                        .query("INSERT INTO thing (label) VALUES (?)", &[json!("alpha")])
                        .await?;
                    txn_ds
                        .query("INSERT INTO thing (label) VALUES (?)", &[json!("beta")])
                        .await?;
                    Ok(json!("done"))
                })
            }))
            .await
            .unwrap();
        assert_eq!(result, json!("done"));
        let rows = ds
            .query("SELECT label FROM thing ORDER BY id", &[])
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn run_in_transaction_rolls_back_on_err() {
        let ds = open_in_memory().await;
        let err = ds
            .run_in_transaction(Box::new(|txn_ds| {
                Box::pin(async move {
                    txn_ds
                        .query("INSERT INTO thing (label) VALUES (?)", &[json!("alpha")])
                        .await?;
                    Err(RepositoryError::Other("rollback please".into()))
                })
            }))
            .await
            .unwrap_err();
        assert!(matches!(err, RepositoryError::Other(_)));
        let rows = ds.query("SELECT label FROM thing", &[]).await.unwrap();
        assert!(rows.is_empty(), "rollback must revert the insert");
    }

    #[test]
    fn is_in_memory_url_matches_known_forms() {
        assert!(is_in_memory_url(":memory:"));
        assert!(is_in_memory_url("sqlite::memory:"));
        assert!(is_in_memory_url("sqlite://:memory:"));
        assert!(is_in_memory_url("SQLite::Memory:"));
        assert!(is_in_memory_url(""));
        assert!(!is_in_memory_url("sqlite:///tmp/app.sqlite"));
        assert!(!is_in_memory_url("sqlite://./app.sqlite?mode=rwc"));
        assert!(!is_in_memory_url("/tmp/app.sqlite"));
    }

    #[tokio::test]
    async fn file_backed_open_enables_wal_journal_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("wal_check.sqlite");
        let url = format!("sqlite://{}", path.display());
        let mut ds = SqliteDatasource::new(SqliteDatasourceOptions {
            url,
            max_connections: 4,
        });
        ds.open().await.unwrap();
        let rows = ds.query("PRAGMA journal_mode", &[]).await.unwrap();
        let mode = rows[0]
            .values()
            .next()
            .and_then(|v| v.as_str())
            .map(str::to_ascii_lowercase)
            .expect("journal_mode value");
        assert_eq!(
            mode, "wal",
            "expected WAL on file-backed sqlite, got {mode}"
        );
    }

    #[tokio::test]
    async fn concurrent_writers_do_not_collide_on_file_backed_sqlite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("concurrent.sqlite");
        let url = format!("sqlite://{}", path.display());
        let mut setup = SqliteDatasource::new(SqliteDatasourceOptions {
            url: url.clone(),
            max_connections: 4,
        });
        setup.open().await.unwrap();
        setup
            .query(
                "CREATE TABLE counter (id INTEGER PRIMARY KEY, n INTEGER NOT NULL)",
                &[],
            )
            .await
            .unwrap();
        setup
            .query("INSERT INTO counter (id, n) VALUES (1, 0)", &[])
            .await
            .unwrap();

        let ds = std::sync::Arc::new(setup);
        let workers = 4usize;
        let iters = 25usize;
        let mut handles = Vec::new();
        for _ in 0..workers {
            let ds = ds.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..iters {
                    ds.query("UPDATE counter SET n = n + 1 WHERE id = 1", &[])
                        .await
                        .expect("concurrent UPDATE must not return database-is-locked");
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let rows = ds
            .query("SELECT n FROM counter WHERE id = 1", &[])
            .await
            .unwrap();
        let n = rows[0].values().next().and_then(|v| v.as_i64()).unwrap();
        assert_eq!(n as usize, workers * iters);
    }

    #[tokio::test]
    async fn concurrent_read_then_write_transactions_do_not_snapshot_conflict() {
        // why: reproduces SQLITE_BUSY_SNAPSHOT (extended code 517). With BEGIN DEFERRED (sqlx default), each txn acquires a read snapshot on its first SELECT and then fails immediately on UPDATE if another connection has committed since — busy_timeout does not retry 517. BEGIN IMMEDIATE takes the writer lock upfront, so workers serialize on the busy_timeout instead.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("snapshot.sqlite");
        let url = format!("sqlite://{}", path.display());
        let mut setup = SqliteDatasource::new(SqliteDatasourceOptions {
            url: url.clone(),
            max_connections: 4,
        });
        setup.open().await.unwrap();
        setup
            .query(
                "CREATE TABLE counter (id INTEGER PRIMARY KEY, n INTEGER NOT NULL)",
                &[],
            )
            .await
            .unwrap();
        setup
            .query("INSERT INTO counter (id, n) VALUES (1, 0)", &[])
            .await
            .unwrap();

        let ds = std::sync::Arc::new(setup);
        let workers = 4usize;
        let iters = 10usize;
        let mut handles = Vec::new();
        for _ in 0..workers {
            let ds = ds.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..iters {
                    ds.run_in_transaction(Box::new(|txn| {
                        Box::pin(async move {
                            let rows = txn.query("SELECT n FROM counter WHERE id = 1", &[]).await?;
                            let n = rows[0].values().next().and_then(|v| v.as_i64()).unwrap();
                            txn.query("UPDATE counter SET n = ? WHERE id = 1", &[json!(n + 1)])
                                .await?;
                            Ok(json!(null))
                        })
                    }))
                    .await
                    .expect("concurrent read-then-write txn must not return SQLITE_BUSY_SNAPSHOT");
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        let rows = ds
            .query("SELECT n FROM counter WHERE id = 1", &[])
            .await
            .unwrap();
        let n = rows[0].values().next().and_then(|v| v.as_i64()).unwrap();
        assert_eq!(n as usize, workers * iters);
    }

    #[tokio::test]
    async fn nested_run_in_transaction_returns_not_supported() {
        let ds = open_in_memory().await;
        let err = ds
            .run_in_transaction(Box::new(|txn_ds| {
                Box::pin(async move {
                    txn_ds
                        .run_in_transaction(Box::new(|_| Box::pin(async { Ok(json!(null)) })))
                        .await
                })
            }))
            .await
            .unwrap_err();
        assert!(
            matches!(err, RepositoryError::TransactionsNotSupported(_)),
            "expected TransactionsNotSupported, got: {err:?}",
        );
    }
}

fn is_in_memory_url(url: &str) -> bool {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    let body = lower
        .strip_prefix("sqlite://")
        .or_else(|| lower.strip_prefix("sqlite:"))
        .unwrap_or(lower.as_str());
    let body = body.split('?').next().unwrap_or(body);
    body == ":memory:" || body.is_empty()
}

fn bind_value<'a>(
    q: sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'a>>,
    v: &'a Value,
) -> sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'a>> {
    match v {
        Value::Null => q.bind(Option::<i64>::None),
        Value::Bool(b) => q.bind(if *b { 1_i64 } else { 0_i64 }),
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
