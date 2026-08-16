use deterministic::repositories::inmemory::InMemoryRepository;
use deterministic::{Repository, RepositoryError};

#[tokio::test]
async fn query_returns_unsupported_error() {
    let repo = InMemoryRepository::new();
    let err = repo.query("SELECT 1", &[]).await.unwrap_err();
    assert!(matches!(err, RepositoryError::UnsupportedQuery(_)));
}
