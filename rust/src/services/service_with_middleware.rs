use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde_json::Value;

use super::dynamic_service::{DynamicService, ServiceError};
use super::service_middleware::ServiceMiddleware;

pub struct ServiceWithMiddleware {
    inner: Arc<dyn DynamicService>,
    chain: Vec<Arc<dyn ServiceMiddleware>>,
    service_name: String,
}

impl ServiceWithMiddleware {
    pub fn wrap(
        inner: Arc<dyn DynamicService>,
        chain: Vec<Arc<dyn ServiceMiddleware>>,
        service_name: impl Into<String>,
    ) -> Arc<dyn DynamicService> {
        if chain.is_empty() {
            return inner;
        }
        Arc::new(Self {
            inner,
            chain,
            service_name: service_name.into(),
        })
    }
}

#[async_trait]
impl DynamicService for ServiceWithMiddleware {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
        for m in &self.chain {
            m.before_call(&self.service_name, method, &args).await;
        }
        let started = Instant::now();
        let result = self.inner.invoke(method, args.clone()).await;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        for m in &self.chain {
            m.after_call(&self.service_name, method, &args, &result, elapsed_ms)
                .await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    use serde_json::json;

    struct RecordingMiddleware {
        tag: String,
        log: Arc<Mutex<Vec<String>>>,
        result_kinds: Arc<Mutex<Vec<&'static str>>>,
        elapsed_observed: Arc<Mutex<Vec<f64>>>,
    }

    impl RecordingMiddleware {
        fn new(
            tag: &str,
            log: Arc<Mutex<Vec<String>>>,
            result_kinds: Arc<Mutex<Vec<&'static str>>>,
            elapsed_observed: Arc<Mutex<Vec<f64>>>,
        ) -> Self {
            Self {
                tag: tag.to_string(),
                log,
                result_kinds,
                elapsed_observed,
            }
        }
    }

    #[async_trait]
    impl ServiceMiddleware for RecordingMiddleware {
        async fn before_call(&self, service_name: &str, method_name: &str, _args: &Value) {
            self.log.lock().unwrap().push(format!(
                "{}:before:{}:{}",
                self.tag, service_name, method_name
            ));
        }

        async fn after_call(
            &self,
            service_name: &str,
            method_name: &str,
            _args: &Value,
            result: &Result<Value, ServiceError>,
            elapsed_ms: f64,
        ) {
            self.log.lock().unwrap().push(format!(
                "{}:after:{}:{}",
                self.tag, service_name, method_name
            ));
            self.result_kinds
                .lock()
                .unwrap()
                .push(if result.is_ok() { "ok" } else { "err" });
            self.elapsed_observed.lock().unwrap().push(elapsed_ms);
        }
    }

    struct InnerService {
        log: Arc<Mutex<Vec<String>>>,
        return_err: bool,
        sleep_ms: u64,
    }

    #[async_trait]
    impl DynamicService for InnerService {
        async fn invoke(&self, method: &str, _args: Value) -> Result<Value, ServiceError> {
            if self.sleep_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.sleep_ms)).await;
            }
            self.log
                .lock()
                .unwrap()
                .push(format!("inner:invoke:{}", method));
            if self.return_err {
                Err(ServiceError::UnknownMethod(method.to_string()))
            } else {
                Ok(json!({ "ran": method }))
            }
        }
    }

    #[tokio::test]
    async fn wrap_runs_before_inner_after_in_chain_order() {
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let kinds = Arc::new(Mutex::new(Vec::new()));
        let elapsed = Arc::new(Mutex::new(Vec::new()));
        let a: Arc<dyn ServiceMiddleware> = Arc::new(RecordingMiddleware::new(
            "a",
            log.clone(),
            kinds.clone(),
            elapsed.clone(),
        ));
        let b: Arc<dyn ServiceMiddleware> = Arc::new(RecordingMiddleware::new(
            "b",
            log.clone(),
            kinds.clone(),
            elapsed.clone(),
        ));
        let inner: Arc<dyn DynamicService> = Arc::new(InnerService {
            log: log.clone(),
            return_err: false,
            sleep_ms: 0,
        });

        let wrapped = ServiceWithMiddleware::wrap(inner, vec![a, b], "WidgetsService");
        let out = wrapped
            .invoke("findById", json!({ "id": 1 }))
            .await
            .expect("ok");
        assert_eq!(out, json!({ "ran": "findById" }));

        let recorded = log.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec![
                "a:before:WidgetsService:findById".to_string(),
                "b:before:WidgetsService:findById".to_string(),
                "inner:invoke:findById".to_string(),
                "a:after:WidgetsService:findById".to_string(),
                "b:after:WidgetsService:findById".to_string(),
            ],
            "chain must run a.before, b.before, inner, a.after, b.after"
        );
    }

    #[tokio::test]
    async fn wrap_with_empty_chain_returns_inner_arc_unchanged() {
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let inner: Arc<dyn DynamicService> = Arc::new(InnerService {
            log: log.clone(),
            return_err: false,
            sleep_ms: 0,
        });
        let wrapped = ServiceWithMiddleware::wrap(inner.clone(), vec![], "Anything");
        assert!(
            Arc::ptr_eq(&inner, &wrapped),
            "empty-chain wrap must return the same Arc<dyn DynamicService> instance"
        );
    }

    #[tokio::test]
    async fn after_call_receives_err_result_when_inner_fails() {
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let kinds = Arc::new(Mutex::new(Vec::new()));
        let elapsed = Arc::new(Mutex::new(Vec::new()));
        let mw: Arc<dyn ServiceMiddleware> = Arc::new(RecordingMiddleware::new(
            "m",
            log.clone(),
            kinds.clone(),
            elapsed.clone(),
        ));
        let inner: Arc<dyn DynamicService> = Arc::new(InnerService {
            log: log.clone(),
            return_err: true,
            sleep_ms: 0,
        });
        let wrapped = ServiceWithMiddleware::wrap(inner, vec![mw], "S");
        let err = wrapped.invoke("nope", json!({})).await.unwrap_err();
        assert!(matches!(err, ServiceError::UnknownMethod(_)));

        let recorded = log.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec![
                "m:before:S:nope".to_string(),
                "inner:invoke:nope".to_string(),
                "m:after:S:nope".to_string(),
            ],
            "after_call must run even when inner errors"
        );
        assert_eq!(kinds.lock().unwrap().clone(), vec!["err"]);
    }

    #[tokio::test]
    async fn after_call_receives_ok_result_when_inner_succeeds() {
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let kinds = Arc::new(Mutex::new(Vec::new()));
        let elapsed = Arc::new(Mutex::new(Vec::new()));
        let mw: Arc<dyn ServiceMiddleware> = Arc::new(RecordingMiddleware::new(
            "m",
            log.clone(),
            kinds.clone(),
            elapsed.clone(),
        ));
        let inner: Arc<dyn DynamicService> = Arc::new(InnerService {
            log: log.clone(),
            return_err: false,
            sleep_ms: 0,
        });
        let wrapped = ServiceWithMiddleware::wrap(inner, vec![mw], "S");
        wrapped.invoke("m", json!({})).await.expect("ok");
        assert_eq!(kinds.lock().unwrap().clone(), vec!["ok"]);
    }

    #[tokio::test]
    async fn elapsed_ms_is_positive_for_non_trivial_inner() {
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let kinds = Arc::new(Mutex::new(Vec::new()));
        let elapsed = Arc::new(Mutex::new(Vec::new()));
        let mw: Arc<dyn ServiceMiddleware> = Arc::new(RecordingMiddleware::new(
            "m",
            log.clone(),
            kinds.clone(),
            elapsed.clone(),
        ));
        let inner: Arc<dyn DynamicService> = Arc::new(InnerService {
            log: log.clone(),
            return_err: false,
            sleep_ms: 5,
        });
        let wrapped = ServiceWithMiddleware::wrap(inner, vec![mw], "S");
        wrapped.invoke("m", json!({})).await.expect("ok");
        let observed = elapsed.lock().unwrap().clone();
        assert_eq!(observed.len(), 1);
        assert!(
            observed[0] > 0.0,
            "elapsed_ms must be > 0 for a non-trivial inner, got {}",
            observed[0]
        );
    }
}
