use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use deterministic::loaders::{GenericRouteSpec, HttpMethod};
use deterministic::repositories::inmemory::InMemoryCrudRepository;
use deterministic::services::DynamicService;
use deterministic::{
    build_router, GenericCrudService, ServiceError, ServiceRegistry, ServiceRegistryBuilder,
};

#[derive(Default, Clone)]
struct CapturedCall {
    method: String,
    args: Value,
}

#[derive(Default, Clone)]
struct RecordingService {
    calls: Arc<Mutex<Vec<CapturedCall>>>,
    response: Arc<Mutex<Value>>,
    error: Arc<Mutex<Option<ServiceError>>>,
}

impl RecordingService {
    fn new(response: Value) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            response: Arc::new(Mutex::new(response)),
            error: Arc::new(Mutex::new(None)),
        }
    }

    fn with_error(mut self, err: ServiceError) -> Self {
        self.error = Arc::new(Mutex::new(Some(err)));
        self
    }

    fn calls(&self) -> Vec<CapturedCall> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl DynamicService for RecordingService {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
        self.calls.lock().unwrap().push(CapturedCall {
            method: method.to_string(),
            args,
        });
        if let Some(err) = self.error.lock().unwrap().take() {
            return Err(err);
        }
        Ok(self.response.lock().unwrap().clone())
    }
}

fn build_registry(name: &str, service: Arc<dyn DynamicService>) -> ServiceRegistry {
    ServiceRegistryBuilder::new()
        .register(name, service)
        .build()
}

#[tokio::test]
async fn registered_post_route_dispatches_body_to_service_method() {
    let svc = Arc::new(RecordingService::new(json!({ "ok": true })));
    let registry = build_registry("ContactImportService", svc.clone());
    let spec = GenericRouteSpec {
        route_name: "import_contacts".to_string(),
        path: "/api/contacts/import".to_string(),
        method: HttpMethod::Post,
        service: "ContactImportService".to_string(),
        service_method: "import".to_string(),
        response_format: None,
        status_code: None,
        aliases: vec![],
    };

    let (router, report) = build_router(&[spec], &registry);
    assert_eq!(report.registered, vec!["import_contacts"]);
    assert!(report.skipped_missing_service.is_empty());

    let req = HttpRequest::builder()
        .method("POST")
        .uri("/api/contacts/import")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"source":"csv","payload":"a,b,c"}"#))
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let body_bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body["ok"], json!(true));

    let calls = svc.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "import");
    assert_eq!(calls[0].args["source"], json!("csv"));
    assert_eq!(calls[0].args["payload"], json!("a,b,c"));
    assert_eq!(calls[0].args["body"]["source"], json!("csv"));
}

#[tokio::test]
async fn registered_get_route_passes_query_to_service_under_query_key() {
    let svc = Arc::new(RecordingService::new(json!([])));
    let registry = build_registry("ListService", svc.clone());
    let spec = GenericRouteSpec {
        route_name: "list".to_string(),
        path: "/api/things".to_string(),
        method: HttpMethod::Get,
        service: "ListService".to_string(),
        service_method: "findAll".to_string(),
        response_format: None,
        status_code: None,
        aliases: vec![],
    };

    let (router, _) = build_router(&[spec], &registry);

    let req = HttpRequest::builder()
        .method("GET")
        .uri("/api/things?limit=10&tag=hot")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let calls = svc.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "findAll");
    assert_eq!(calls[0].args["query"]["limit"], json!("10"));
    assert_eq!(calls[0].args["query"]["tag"], json!("hot"));
}

#[tokio::test]
async fn unknown_method_returns_405() {
    let svc = Arc::new(
        RecordingService::new(Value::Null).with_error(ServiceError::UnknownMethod("nope".into())),
    );
    let registry = build_registry("S", svc.clone());
    let spec = GenericRouteSpec {
        route_name: "r".to_string(),
        path: "/r".to_string(),
        method: HttpMethod::Post,
        service: "S".to_string(),
        service_method: "nope".to_string(),
        response_format: None,
        status_code: None,
        aliases: vec![],
    };

    let (router, _) = build_router(&[spec], &registry);
    let req = HttpRequest::builder()
        .method("POST")
        .uri("/r")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn invalid_args_returns_400() {
    let svc = Arc::new(
        RecordingService::new(Value::Null).with_error(ServiceError::InvalidArgs("missing".into())),
    );
    let registry = build_registry("S", svc.clone());
    let spec = GenericRouteSpec {
        route_name: "r".to_string(),
        path: "/r".to_string(),
        method: HttpMethod::Post,
        service: "S".to_string(),
        service_method: "x".to_string(),
        response_format: None,
        status_code: None,
        aliases: vec![],
    };

    let (router, _) = build_router(&[spec], &registry);
    let req = HttpRequest::builder()
        .method("POST")
        .uri("/r")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unregistered_service_skips_route_with_report() {
    let registry = ServiceRegistry::new();
    let spec = GenericRouteSpec {
        route_name: "orphan".to_string(),
        path: "/orphan".to_string(),
        method: HttpMethod::Post,
        service: "NotRegistered".to_string(),
        service_method: "x".to_string(),
        response_format: None,
        status_code: None,
        aliases: vec![],
    };

    let (router, report) = build_router(&[spec], &registry);
    assert!(report.registered.is_empty());
    assert_eq!(report.skipped_missing_service, vec!["orphan"]);

    let req = HttpRequest::builder()
        .method("POST")
        .uri("/orphan")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn generic_crud_service_via_router_round_trip() {
    let svc: Arc<dyn DynamicService> = Arc::new(GenericCrudService::new(Arc::new(
        InMemoryCrudRepository::new(),
    )));
    let registry = build_registry("widget", svc);
    let create = GenericRouteSpec {
        route_name: "create_widget".to_string(),
        path: "/api/widgets".to_string(),
        method: HttpMethod::Post,
        service: "widget".to_string(),
        service_method: "create".to_string(),
        response_format: None,
        status_code: None,
        aliases: vec![],
    };
    let list = GenericRouteSpec {
        route_name: "list_widgets".to_string(),
        path: "/api/widgets".to_string(),
        method: HttpMethod::Get,
        service: "widget".to_string(),
        service_method: "findAll".to_string(),
        response_format: None,
        status_code: None,
        aliases: vec![],
    };

    let (router, report) = build_router(&[create, list], &registry);
    assert_eq!(report.registered.len(), 2);

    let post = HttpRequest::builder()
        .method("POST")
        .uri("/api/widgets")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"label":"alpha"}"#))
        .unwrap();
    let res = router.clone().oneshot(post).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["label"], json!("alpha"));
    assert!(body["id"].is_number());

    let get = HttpRequest::builder()
        .method("GET")
        .uri("/api/widgets")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(get).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["label"], json!("alpha"));
}
