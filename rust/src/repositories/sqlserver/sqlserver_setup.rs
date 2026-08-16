use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::setup::Setup;
use crate::repositories::sqlserver::sqlserver_datasource::SqlserverDatasource;

#[derive(Debug, Clone, Default)]
pub struct SqlserverSetupOptions {
    pub setup_sql_path: Option<PathBuf>,
    pub seed_sql_path: Option<PathBuf>,
}

pub struct SqlserverSetup {
    datasource: Arc<SqlserverDatasource>,
    options: SqlserverSetupOptions,
}

impl SqlserverSetup {
    pub fn new(datasource: Arc<SqlserverDatasource>, options: SqlserverSetupOptions) -> Self {
        Self {
            datasource,
            options,
        }
    }

    pub fn options(&self) -> &SqlserverSetupOptions {
        &self.options
    }

    pub fn datasource(&self) -> &Arc<SqlserverDatasource> {
        &self.datasource
    }
}

#[async_trait]
impl Setup for SqlserverSetup {
    async fn run(&self) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "SqlserverSetup pending tiberius wiring",
        ))
    }
}
