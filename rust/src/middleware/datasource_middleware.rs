use async_trait::async_trait;
use serde_json::Value;

use crate::error::RepositoryError;

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
