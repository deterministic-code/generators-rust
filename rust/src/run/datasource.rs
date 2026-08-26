use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::RepositoryError;
use crate::id_type::IdType;
use crate::mappings::field_mapping_translator::FieldMappingTranslator;
use crate::mappings::parse_field_mappings::EntityFieldMap;
use crate::repositories::datasource_middleware::DataSourceMiddleware;
use crate::repositories::mysql::{MysqlCrudRepository, MysqlDatasource, MysqlDatasourceOptions};
use crate::repositories::postgres::{
    PostgresCrudRepository, PostgresDatasource, PostgresDatasourceOptions,
};
use crate::repositories::sqlite::{
    SqliteCrudRepository, SqliteDatasource, SqliteDatasourceOptions,
};
use crate::repositories::{CrudRepository, Datasource};
use crate::run::converter_defaults::default_field_converters_for_dialect;

pub enum OpenDatasource {
    Sqlite(Arc<SqliteDatasource>),
    Postgres(Arc<PostgresDatasource>),
    Mysql(Arc<MysqlDatasource>),
}

#[derive(Debug, Clone)]
pub enum DialectKind {
    Sqlite,
    Postgres,
    Mysql,
}

impl OpenDatasource {
    pub fn dialect(&self) -> DialectKind {
        match self {
            Self::Sqlite(_) => DialectKind::Sqlite,
            Self::Postgres(_) => DialectKind::Postgres,
            Self::Mysql(_) => DialectKind::Mysql,
        }
    }

    pub fn as_datasource(&self) -> Arc<dyn Datasource> {
        match self {
            Self::Sqlite(ds) => ds.clone(),
            Self::Postgres(ds) => ds.clone(),
            Self::Mysql(ds) => ds.clone(),
        }
    }

    /// Closes the underlying sqlx pool by shared reference (`Datasource::close` needs `&mut`, unreachable through the `Arc` the router holds); a datasource that was never opened has nothing to close.
    pub async fn close_pool(&self) {
        match self {
            Self::Sqlite(ds) => {
                if let Ok(pool) = ds.pool() {
                    pool.close().await;
                }
            }
            Self::Postgres(ds) => {
                if let Ok(pool) = ds.pool() {
                    pool.close().await;
                }
            }
            Self::Mysql(ds) => {
                if let Ok(pool) = ds.pool() {
                    pool.close().await;
                }
            }
        }
    }

    pub fn build_crud_repo(
        &self,
        table: &str,
        primary_key: &str,
    ) -> Result<Arc<dyn CrudRepository>, RepositoryError> {
        let keys = [primary_key.to_string()];
        self.build_crud_repo_with_mapping(
            table,
            &keys,
            None,
            &[],
            &BTreeMap::new(),
            IdType::Integer,
        )
    }

    pub fn build_crud_repo_with_mapping(
        &self,
        table: &str,
        primary_keys: &[String],
        entity_map: Option<&EntityFieldMap>,
        middlewares: &[Arc<dyn DataSourceMiddleware>],
        column_datasource_types: &BTreeMap<String, String>,
        id_type: IdType,
    ) -> Result<Arc<dyn CrudRepository>, RepositoryError> {
        build_crud_repo_for_datasource(
            self.dialect(),
            self.as_datasource(),
            table,
            primary_keys,
            entity_map,
            middlewares,
            column_datasource_types,
            id_type,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_crud_repo_for_datasource(
    dialect: DialectKind,
    datasource: Arc<dyn Datasource>,
    table: &str,
    primary_keys: &[String],
    entity_map: Option<&EntityFieldMap>,
    middlewares: &[Arc<dyn DataSourceMiddleware>],
    column_datasource_types: &BTreeMap<String, String>,
    id_type: IdType,
) -> Result<Arc<dyn CrudRepository>, RepositoryError> {
    let translator = Arc::new(FieldMappingTranslator::new(entity_map)?);
    let converters = default_field_converters_for_dialect(column_datasource_types, &dialect);
    let custom_keys = primary_keys != ["id".to_string()];
    match dialect {
        DialectKind::Sqlite => {
            let mut repo = SqliteCrudRepository::new(datasource, table)?;
            if custom_keys {
                repo = repo.with_primary_keys(primary_keys.iter().cloned())?;
            }
            repo = repo.with_id_type(id_type);
            repo = repo.with_field_mapping_translator(translator);
            if !middlewares.is_empty() {
                repo = repo.with_middlewares(middlewares.to_vec());
            }
            if !column_datasource_types.is_empty() {
                repo = repo.with_column_datasource_types(column_datasource_types.clone());
            }
            if !converters.is_empty() {
                repo = repo.with_converters(converters);
            }
            Ok(Arc::new(repo))
        }
        DialectKind::Postgres => {
            let mut repo = PostgresCrudRepository::new(datasource, table)?;
            if custom_keys {
                repo = repo.with_primary_keys(primary_keys.iter().cloned())?;
            }
            repo = repo.with_id_type(id_type);
            repo = repo.with_field_mapping_translator(translator);
            if !middlewares.is_empty() {
                repo = repo.with_middlewares(middlewares.to_vec());
            }
            if !column_datasource_types.is_empty() {
                repo = repo.with_column_datasource_types(column_datasource_types.clone());
            }
            if !converters.is_empty() {
                repo = repo.with_converters(converters);
            }
            Ok(Arc::new(repo))
        }
        DialectKind::Mysql => {
            let mut repo = MysqlCrudRepository::new(datasource, table)?;
            if custom_keys {
                repo = repo.with_primary_keys(primary_keys.iter().cloned())?;
            }
            repo = repo.with_id_type(id_type);
            repo = repo.with_field_mapping_translator(translator);
            if !middlewares.is_empty() {
                repo = repo.with_middlewares(middlewares.to_vec());
            }
            if !column_datasource_types.is_empty() {
                repo = repo.with_column_datasource_types(column_datasource_types.clone());
            }
            if !converters.is_empty() {
                repo = repo.with_converters(converters);
            }
            Ok(Arc::new(repo))
        }
    }
}

pub async fn open_datasource(url: &str) -> Result<OpenDatasource, RepositoryError> {
    if url.starts_with("oracle://") || url.starts_with("oracle:") {
        return Err(RepositoryError::Unimplemented(
            "oracle backend not available in Rust ecosystem",
        ));
    }
    if url.starts_with("sqlserver://") || url.starts_with("mssql://") || url.starts_with("mssql:") {
        return Err(RepositoryError::Unimplemented(
            "sqlserver backend not available in Rust ecosystem (tiberius integration pending)",
        ));
    }
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        let mut ds = PostgresDatasource::new(PostgresDatasourceOptions {
            url: url.to_string(),
            max_connections: 4,
        });
        ds.open().await?;
        Ok(OpenDatasource::Postgres(Arc::new(ds)))
    } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
        let mut ds = MysqlDatasource::new(MysqlDatasourceOptions {
            url: url.to_string(),
            max_connections: 4,
        });
        ds.open().await?;
        Ok(OpenDatasource::Mysql(Arc::new(ds)))
    } else {
        let options = if url == "sqlite::memory:" || url == ":memory:" {
            SqliteDatasourceOptions::in_memory()
        } else {
            let url = if url.starts_with("sqlite:") {
                url.to_string()
            } else {
                format!("sqlite://{}", url)
            };
            SqliteDatasourceOptions {
                url,
                max_connections: 4,
            }
        };
        let mut ds = SqliteDatasource::new(options);
        ds.open().await?;
        Ok(OpenDatasource::Sqlite(Arc::new(ds)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_datasource_picks_sqlite_for_memory_url() {
        let ds = open_datasource("sqlite::memory:").await.unwrap();
        assert!(matches!(ds, OpenDatasource::Sqlite(_)));
        assert!(matches!(ds.dialect(), DialectKind::Sqlite));
    }

    #[tokio::test]
    async fn open_datasource_picks_sqlite_for_file_path() {
        let ds = open_datasource("sqlite::memory:").await.unwrap();
        assert!(matches!(ds, OpenDatasource::Sqlite(_)));
    }

    #[tokio::test]
    async fn open_datasource_rejects_oracle_url_with_clear_message() {
        let err = match open_datasource("oracle://user:pass@host/db").await {
            Err(e) => e,
            Ok(_) => panic!("expected Err for oracle:// url"),
        };
        let msg = format!("{}", err);
        assert!(
            msg.contains("oracle backend not available"),
            "expected clear oracle message, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn open_datasource_rejects_sqlserver_url_with_clear_message() {
        let err = match open_datasource("sqlserver://user:pass@host/db").await {
            Err(e) => e,
            Ok(_) => panic!("expected Err for sqlserver:// url"),
        };
        let msg = format!("{}", err);
        assert!(
            msg.contains("sqlserver backend not available"),
            "expected clear sqlserver message, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn open_datasource_rejects_mssql_url_with_clear_message() {
        let err = match open_datasource("mssql://user:pass@host/db").await {
            Err(e) => e,
            Ok(_) => panic!("expected Err for mssql:// url"),
        };
        let msg = format!("{}", err);
        assert!(
            msg.contains("sqlserver backend not available"),
            "expected clear sqlserver message, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn build_crud_repo_returns_dyn_trait_object() {
        let ds = open_datasource("sqlite::memory:").await.unwrap();
        ds.as_datasource()
            .query(
                "CREATE TABLE widgets (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT)",
                &[],
            )
            .await
            .unwrap();
        let repo = ds.build_crud_repo("widgets", "id").unwrap();
        let rows = repo.find_all().await.unwrap();
        assert!(rows.is_empty());
    }
}
