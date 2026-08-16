use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::datasource::Datasource;
use crate::repositories::postgres::postgres_datasource::PostgresDatasource;
use crate::repositories::setup::Setup;

#[derive(Debug, Clone)]
pub struct PostgresSetupOptions {
    pub setup_sql_path: Option<PathBuf>,
    pub seed_sql_path: Option<PathBuf>,
}

pub struct PostgresSetup {
    datasource: Arc<PostgresDatasource>,
    options: PostgresSetupOptions,
}

impl PostgresSetup {
    pub fn new(datasource: Arc<PostgresDatasource>, options: PostgresSetupOptions) -> Self {
        Self {
            datasource,
            options,
        }
    }
}

#[async_trait]
impl Setup for PostgresSetup {
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
                self.datasource.query(&sql, &[]).await?;
            }
        }
        Ok(())
    }
}
