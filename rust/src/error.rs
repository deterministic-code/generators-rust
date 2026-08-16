use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("invalid SQL identifier: {0}")]
    InvalidIdentifier(String),

    #[error("datasource is not open; call open() first")]
    NotOpen,

    #[error("backend does not support raw SQL queries: {0}")]
    UnsupportedQuery(String),

    #[error("unimplemented: {0}")]
    Unimplemented(&'static str),

    #[error("inserted row id={0} not found")]
    InsertedRowMissing(i64),

    #[error("invalid repository configuration: {0}")]
    InvalidConfig(String),

    #[error("optimistic concurrency conflict: {0}")]
    ConcurrencyConflict(String),

    #[error("transactions not supported: {0}")]
    TransactionsNotSupported(String),

    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("other: {0}")]
    Other(String),
}
