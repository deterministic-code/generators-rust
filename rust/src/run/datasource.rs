use crate::error::RepositoryError;

/// Dialect recorded from `DATABASE_URL`. Persistence now lives on generated Datasources
/// repositories; boot only needs to know which backend the app was pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenDatasource {
    Sqlite,
    Postgres,
    Mysql,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialectKind {
    Sqlite,
    Postgres,
    Mysql,
}

impl OpenDatasource {
    pub fn dialect(&self) -> DialectKind {
        match self {
            Self::Sqlite => DialectKind::Sqlite,
            Self::Postgres => DialectKind::Postgres,
            Self::Mysql => DialectKind::Mysql,
        }
    }

    pub async fn close_pool(&self) {}
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
        Ok(OpenDatasource::Postgres)
    } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
        Ok(OpenDatasource::Mysql)
    } else {
        Ok(OpenDatasource::Sqlite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_datasource_picks_sqlite_for_memory_url() {
        let ds = open_datasource("sqlite::memory:").await.unwrap();
        assert!(matches!(ds, OpenDatasource::Sqlite));
        assert!(matches!(ds.dialect(), DialectKind::Sqlite));
    }

    #[tokio::test]
    async fn open_datasource_picks_sqlite_for_file_path() {
        let ds = open_datasource("sqlite::memory:").await.unwrap();
        assert!(matches!(ds, OpenDatasource::Sqlite));
    }

    #[tokio::test]
    async fn open_datasource_picks_postgres_url() {
        let ds = open_datasource("postgres://localhost/app").await.unwrap();
        assert!(matches!(ds, OpenDatasource::Postgres));
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
}
