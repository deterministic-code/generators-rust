use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RepositoryError;
use crate::repositories::datasource::Datasource;
use crate::repositories::row_map::RowMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseBackend {
    Sqlite,
    Postgres,
    Mysql,
    Sqlserver,
    Oracle,
}

impl DatabaseBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            DatabaseBackend::Sqlite => "sqlite",
            DatabaseBackend::Postgres => "postgres",
            DatabaseBackend::Mysql => "mysql",
            DatabaseBackend::Sqlserver => "sqlserver",
            DatabaseBackend::Oracle => "oracle",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RewrittenQuery {
    pub query: String,
    pub params: Vec<Value>,
}

#[async_trait]
pub trait DataSourceMiddleware: Send + Sync {
    async fn before_query(
        &self,
        provider: DatabaseBackend,
        query: &str,
        params: Option<&[Value]>,
    ) -> Option<RewrittenQuery>;

    async fn after_query(
        &self,
        provider: DatabaseBackend,
        query: &str,
        params: Option<&[Value]>,
        results: &[Value],
        elapsed_ms: f64,
        error: Option<&RepositoryError>,
    );
}

pub async fn run_query_with_middlewares(
    middlewares: &[Arc<dyn DataSourceMiddleware>],
    backend: DatabaseBackend,
    datasource: &dyn Datasource,
    sql: &str,
    params: &[Value],
) -> Result<Vec<RowMap>, RepositoryError> {
    if middlewares.is_empty() {
        return datasource.query(sql, params).await;
    }

    let mut current_sql = sql.to_string();
    let mut current_params: Vec<Value> = params.to_vec();
    for m in middlewares {
        if let Some(rewritten) = m
            .before_query(backend, &current_sql, Some(&current_params))
            .await
        {
            current_sql = rewritten.query;
            current_params = rewritten.params;
        }
    }

    let start = Instant::now();
    let result = datasource.query(&current_sql, &current_params).await;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    let results_as_values: Vec<Value> = match &result {
        Ok(rows) => rows
            .iter()
            .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
            .collect(),
        Err(_) => Vec::new(),
    };
    let error_ref = result.as_ref().err();
    for m in middlewares {
        m.after_query(
            backend,
            &current_sql,
            Some(&current_params),
            &results_as_values,
            elapsed_ms,
            error_ref,
        )
        .await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    pub struct RecordingDataSourceMiddleware {
        pub before_calls: Mutex<Vec<(DatabaseBackend, String, Option<Vec<Value>>)>>,
        pub after_calls: Mutex<Vec<AfterQueryRecord>>,
        pub rewrite_to: Option<RewrittenQuery>,
    }

    pub struct AfterQueryRecord {
        pub provider: DatabaseBackend,
        pub query: String,
        pub params: Option<Vec<Value>>,
        pub elapsed_ms: f64,
        pub error_message: Option<String>,
    }

    impl Default for RecordingDataSourceMiddleware {
        fn default() -> Self {
            Self {
                before_calls: Mutex::new(Vec::new()),
                after_calls: Mutex::new(Vec::new()),
                rewrite_to: None,
            }
        }
    }

    #[async_trait]
    impl DataSourceMiddleware for RecordingDataSourceMiddleware {
        async fn before_query(
            &self,
            provider: DatabaseBackend,
            query: &str,
            params: Option<&[Value]>,
        ) -> Option<RewrittenQuery> {
            self.before_calls.lock().unwrap().push((
                provider,
                query.to_string(),
                params.map(|p| p.to_vec()),
            ));
            self.rewrite_to.clone()
        }

        async fn after_query(
            &self,
            provider: DatabaseBackend,
            query: &str,
            params: Option<&[Value]>,
            _results: &[Value],
            elapsed_ms: f64,
            error: Option<&RepositoryError>,
        ) {
            self.after_calls.lock().unwrap().push(AfterQueryRecord {
                provider,
                query: query.to_string(),
                params: params.map(|p| p.to_vec()),
                elapsed_ms,
                error_message: error.map(|e| e.to_string()),
            });
        }
    }

    #[tokio::test]
    async fn before_query_returns_none_by_default_for_passthrough() {
        let m = RecordingDataSourceMiddleware::default();
        let out = m
            .before_query(DatabaseBackend::Sqlite, "SELECT 1", None)
            .await;
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn before_query_returns_some_when_rewriter_present() {
        let m = RecordingDataSourceMiddleware {
            rewrite_to: Some(RewrittenQuery {
                query: "SELECT 2".to_string(),
                params: vec![],
            }),
            ..Default::default()
        };
        let out = m
            .before_query(DatabaseBackend::Sqlite, "SELECT 1", None)
            .await
            .expect("rewrite");
        assert_eq!(out.query, "SELECT 2");
    }

    #[tokio::test]
    async fn after_query_captures_provider_query_params_elapsed_and_error() {
        let m = RecordingDataSourceMiddleware::default();
        let err = RepositoryError::Other("boom".to_string());
        m.after_query(
            DatabaseBackend::Postgres,
            "INSERT INTO t VALUES ($1)",
            Some(&[Value::from(42)]),
            &[],
            12.5,
            Some(&err),
        )
        .await;
        let calls = m.after_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].provider, DatabaseBackend::Postgres);
        assert_eq!(calls[0].query, "INSERT INTO t VALUES ($1)");
        assert_eq!(
            calls[0].params.as_deref(),
            Some([Value::from(42)].as_slice())
        );
        assert_eq!(calls[0].elapsed_ms, 12.5);
        assert_eq!(calls[0].error_message.as_deref(), Some("other: boom"));
    }

    #[tokio::test]
    async fn trait_is_object_safe_via_arc_dyn() {
        let m: Arc<dyn DataSourceMiddleware> = Arc::new(RecordingDataSourceMiddleware::default());
        let _ = m
            .before_query(DatabaseBackend::Mysql, "SELECT 1", None)
            .await;
    }

    #[test]
    fn database_backend_as_str_matches_ts_lowercase_names() {
        assert_eq!(DatabaseBackend::Sqlite.as_str(), "sqlite");
        assert_eq!(DatabaseBackend::Postgres.as_str(), "postgres");
        assert_eq!(DatabaseBackend::Mysql.as_str(), "mysql");
        assert_eq!(DatabaseBackend::Sqlserver.as_str(), "sqlserver");
        assert_eq!(DatabaseBackend::Oracle.as_str(), "oracle");
    }
}
