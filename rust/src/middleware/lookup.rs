use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::repositories::datasource_middleware::DataSourceMiddleware;
use crate::services::ServiceMiddleware;

use super::data_source_console_logger_middleware::DataSourceConsoleLoggerMiddleware;
use super::service_console_logger_middleware::ServiceConsoleLoggerMiddleware;

#[derive(Debug, Default, Clone)]
pub struct MiddlewareDeps {}

#[derive(Debug, Error)]
pub enum LookupError {
    #[error("Unknown service middleware: {0}")]
    UnknownService(String),
    #[error("Unknown datasource middleware: {0}")]
    UnknownDatasource(String),
}

pub type ServiceMiddlewareFactory =
    Arc<dyn Fn(&MiddlewareDeps) -> Arc<dyn ServiceMiddleware> + Send + Sync>;

pub type DataSourceMiddlewareFactory =
    Arc<dyn Fn(&MiddlewareDeps) -> Arc<dyn DataSourceMiddleware> + Send + Sync>;

pub struct ServiceMiddlewareLookup {
    deps: MiddlewareDeps,
    factories: HashMap<String, ServiceMiddlewareFactory>,
    trace_writer: Mutex<Option<Box<dyn Write + Send>>>,
}

impl ServiceMiddlewareLookup {
    pub fn new(deps: MiddlewareDeps, factories: HashMap<String, ServiceMiddlewareFactory>) -> Self {
        Self {
            deps,
            factories,
            trace_writer: Mutex::new(None),
        }
    }

    pub fn empty() -> Self {
        Self::new(MiddlewareDeps::default(), HashMap::new())
    }

    pub fn with_trace_writer(self, writer: Box<dyn Write + Send>) -> Self {
        Self {
            trace_writer: Mutex::new(Some(writer)),
            ..self
        }
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn ServiceMiddleware>, LookupError> {
        match name {
            "traceService" => {
                let taken = self.trace_writer.lock().unwrap().take();
                Ok(Arc::new(match taken {
                    Some(w) => ServiceConsoleLoggerMiddleware::with_writer(w),
                    None => ServiceConsoleLoggerMiddleware::new(),
                }))
            }
            _ => match self.factories.get(name) {
                Some(factory) => Ok(factory(&self.deps)),
                None => Err(LookupError::UnknownService(name.to_string())),
            },
        }
    }
}

pub struct DataSourceMiddlewareLookup {
    deps: MiddlewareDeps,
    factories: HashMap<String, DataSourceMiddlewareFactory>,
    trace_writer: Mutex<Option<Box<dyn Write + Send>>>,
}

impl DataSourceMiddlewareLookup {
    pub fn new(
        deps: MiddlewareDeps,
        factories: HashMap<String, DataSourceMiddlewareFactory>,
    ) -> Self {
        Self {
            deps,
            factories,
            trace_writer: Mutex::new(None),
        }
    }

    pub fn empty() -> Self {
        Self::new(MiddlewareDeps::default(), HashMap::new())
    }

    pub fn with_trace_writer(self, writer: Box<dyn Write + Send>) -> Self {
        Self {
            trace_writer: Mutex::new(Some(writer)),
            ..self
        }
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn DataSourceMiddleware>, LookupError> {
        match name {
            "dataSourceConsoleLogger" | "traceDatasource" => {
                let taken = self.trace_writer.lock().unwrap().take();
                Ok(Arc::new(match taken {
                    Some(w) => DataSourceConsoleLoggerMiddleware::with_writer(w),
                    None => DataSourceConsoleLoggerMiddleware::new(),
                }))
            }
            _ => match self.factories.get(name) {
                Some(factory) => Ok(factory(&self.deps)),
                None => Err(LookupError::UnknownDatasource(name.to_string())),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use serde_json::Value;

    use crate::error::RepositoryError;
    use crate::repositories::datasource_middleware::{DatabaseBackend, RewrittenQuery};
    use crate::services::ServiceError;

    struct FakeServiceMiddleware;

    #[async_trait]
    impl ServiceMiddleware for FakeServiceMiddleware {
        async fn before_call(&self, _svc: &str, _method: &str, _args: &Value) {}
        async fn after_call(
            &self,
            _svc: &str,
            _method: &str,
            _args: &Value,
            _result: &Result<Value, ServiceError>,
            _elapsed_ms: f64,
        ) {
        }
    }

    struct FakeDataSourceMiddleware;

    #[async_trait]
    impl DataSourceMiddleware for FakeDataSourceMiddleware {
        async fn before_query(
            &self,
            _provider: DatabaseBackend,
            _query: &str,
            _params: Option<&[Value]>,
        ) -> Option<RewrittenQuery> {
            None
        }
        async fn after_query(
            &self,
            _provider: DatabaseBackend,
            _query: &str,
            _params: Option<&[Value]>,
            _results: &[Value],
            _elapsed_ms: f64,
            _error: Option<&RepositoryError>,
        ) {
        }
    }

    #[test]
    fn service_lookup_resolves_trace_service_builtin() {
        let lookup = ServiceMiddlewareLookup::empty();
        assert!(lookup.get("traceService").is_ok());
    }

    #[test]
    fn datasource_lookup_resolves_trace_datasource_builtin() {
        let lookup = DataSourceMiddlewareLookup::empty();
        assert!(lookup.get("traceDatasource").is_ok());
    }

    #[test]
    fn datasource_lookup_resolves_data_source_console_logger_builtin() {
        let lookup = DataSourceMiddlewareLookup::empty();
        assert!(lookup.get("dataSourceConsoleLogger").is_ok());
    }

    #[test]
    fn service_lookup_resolves_factory_contributed_name() {
        let mut factories: HashMap<String, ServiceMiddlewareFactory> = HashMap::new();
        factories.insert(
            "myAudit".to_string(),
            Arc::new(|_deps| Arc::new(FakeServiceMiddleware) as Arc<dyn ServiceMiddleware>),
        );
        let lookup = ServiceMiddlewareLookup::new(MiddlewareDeps::default(), factories);
        assert!(lookup.get("myAudit").is_ok());
    }

    #[test]
    fn datasource_lookup_resolves_factory_contributed_name() {
        let mut factories: HashMap<String, DataSourceMiddlewareFactory> = HashMap::new();
        factories.insert(
            "myRewriter".to_string(),
            Arc::new(|_deps| Arc::new(FakeDataSourceMiddleware) as Arc<dyn DataSourceMiddleware>),
        );
        let lookup = DataSourceMiddlewareLookup::new(MiddlewareDeps::default(), factories);
        assert!(lookup.get("myRewriter").is_ok());
    }

    #[test]
    fn service_lookup_builtin_wins_over_factory_with_same_name() {
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let mut factories: HashMap<String, ServiceMiddlewareFactory> = HashMap::new();
        factories.insert(
            "traceService".to_string(),
            Arc::new(move |_deps| {
                flag.store(true, Ordering::SeqCst);
                Arc::new(FakeServiceMiddleware) as Arc<dyn ServiceMiddleware>
            }),
        );
        let lookup = ServiceMiddlewareLookup::new(MiddlewareDeps::default(), factories);
        let _ = lookup.get("traceService").expect("builtin resolves");
        assert!(
            !called.load(Ordering::SeqCst),
            "factory closure must not be invoked when name matches a built-in"
        );
    }

    #[test]
    fn datasource_lookup_builtin_wins_over_factory_with_same_name() {
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let mut factories: HashMap<String, DataSourceMiddlewareFactory> = HashMap::new();
        factories.insert(
            "traceDatasource".to_string(),
            Arc::new(move |_deps| {
                flag.store(true, Ordering::SeqCst);
                Arc::new(FakeDataSourceMiddleware) as Arc<dyn DataSourceMiddleware>
            }),
        );
        let lookup = DataSourceMiddlewareLookup::new(MiddlewareDeps::default(), factories);
        let _ = lookup.get("traceDatasource").expect("builtin resolves");
        assert!(
            !called.load(Ordering::SeqCst),
            "factory closure must not be invoked when name matches a built-in"
        );
    }

    #[test]
    fn service_lookup_unknown_name_returns_unknown_service_error() {
        let lookup = ServiceMiddlewareLookup::empty();
        match lookup.get("nope") {
            Err(LookupError::UnknownService(n)) => assert_eq!(n, "nope"),
            other => panic!("expected UnknownService(\"nope\"), got {:?}", other.err()),
        }
    }

    #[test]
    fn datasource_lookup_unknown_name_returns_unknown_datasource_error() {
        let lookup = DataSourceMiddlewareLookup::empty();
        match lookup.get("nope") {
            Err(LookupError::UnknownDatasource(n)) => assert_eq!(n, "nope"),
            other => panic!(
                "expected UnknownDatasource(\"nope\"), got {:?}",
                other.err()
            ),
        }
    }
}
