use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::datasource::Datasource;
use crate::repositories::setup::Setup;
use crate::repositories::sqlite::sqlite_datasource::SqliteDatasource;

#[derive(Debug, Clone)]
pub struct SqliteSetupOptions {
    pub setup_sql_path: Option<PathBuf>,
    pub seed_sql_path: Option<PathBuf>,
}

pub struct SqliteSetup {
    datasource: Arc<SqliteDatasource>,
    options: SqliteSetupOptions,
}

impl SqliteSetup {
    pub fn new(datasource: Arc<SqliteDatasource>, options: SqliteSetupOptions) -> Self {
        Self {
            datasource,
            options,
        }
    }
}

#[async_trait]
impl Setup for SqliteSetup {
    async fn run(&self) -> Result<(), RepositoryError> {
        if let Some(path) = &self.options.setup_sql_path {
            if path.exists() {
                let sql = std::fs::read_to_string(path)?;
                self.datasource.query(&sql, &[]).await?;
            }
        }
        if let Some(path) = &self.options.seed_sql_path {
            if path.exists() {
                let sql = std::fs::read_to_string(path)?;
                let rewritten = sql.replace("INSERT INTO", "INSERT OR IGNORE INTO");
                self.datasource.query(&rewritten, &[]).await?;
            }
        }
        Ok(())
    }
}
