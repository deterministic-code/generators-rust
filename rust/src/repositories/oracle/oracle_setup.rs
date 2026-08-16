use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::repositories::oracle::oracle_datasource::OracleDatasource;
use crate::repositories::setup::Setup;

#[derive(Debug, Clone, Default)]
pub struct OracleSetupOptions {
    pub setup_sql_path: Option<PathBuf>,
    pub seed_sql_path: Option<PathBuf>,
}

pub struct OracleSetup {
    _datasource: Arc<OracleDatasource>,
    options: OracleSetupOptions,
}

impl OracleSetup {
    pub fn new(datasource: Arc<OracleDatasource>, options: OracleSetupOptions) -> Self {
        Self {
            _datasource: datasource,
            options,
        }
    }

    pub fn options(&self) -> &OracleSetupOptions {
        &self.options
    }
}

#[async_trait]
impl Setup for OracleSetup {
    async fn run(&self) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unimplemented(
            "oracle backend not available in Rust ecosystem",
        ))
    }
}
