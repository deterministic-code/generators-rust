use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::datasource::Datasource;
use crate::repositories::mysql::mysql_datasource::MysqlDatasource;
use crate::repositories::setup::Setup;

#[derive(Debug, Clone)]
pub struct MysqlSetupOptions {
    pub setup_sql_path: Option<PathBuf>,
    pub seed_sql_path: Option<PathBuf>,
}

pub struct MysqlSetup {
    datasource: Arc<MysqlDatasource>,
    options: MysqlSetupOptions,
}

impl MysqlSetup {
    pub fn new(datasource: Arc<MysqlDatasource>, options: MysqlSetupOptions) -> Self {
        Self {
            datasource,
            options,
        }
    }
}

#[async_trait]
impl Setup for MysqlSetup {
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
