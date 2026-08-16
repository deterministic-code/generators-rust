use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use deterministic::middleware::{trace_route_layer, TraceRouteMiddleware};
use http_body_util::BodyExt;
use tower::ServiceExt;

struct VecWriter(Arc<Mutex<Vec<u8>>>);

impl Write for VecWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn buffer_pair() -> (Arc<Mutex<Vec<u8>>>, Box<dyn Write + Send>) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = Box::new(VecWriter(buf.clone()));
    (buf, writer)
}

fn captured(buf: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

#[tokio::test]
async fn emit_main_rs_runtime_block_attaches_layer_when_enabled() {
    let (buf, writer) = buffer_pair();
    let logger = Arc::new(TraceRouteMiddleware::with_writer(writer));
    let app: Router = Router::new()
        .route("/api/users", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(
            logger,
            trace_route_layer,
        ));
    let req = Request::builder()
        .method("GET")
        .uri("/api/users")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let _ = res.into_body().collect().await.unwrap();
    let out = captured(&buf);
    assert!(
        out.contains("[route][Start]-[GET /api/users]"),
        "expected start line, got: {}",
        out
    );
    assert!(
        out.contains("[route][Finish]-[GET /api/users] 200 "),
        "expected finish 200 line, got: {}",
        out
    );
}

#[tokio::test]
async fn router_without_layer_produces_no_trace_output_regression_guard() {
    let (buf, _writer) = buffer_pair();
    let app: Router = Router::new().route("/api/users", get(|| async { "ok" }));
    let req = Request::builder()
        .method("GET")
        .uri("/api/users")
        .body(Body::empty())
        .unwrap();
    let _ = app.oneshot(req).await.unwrap();
    let out = captured(&buf);
    assert!(
        !out.contains("[route]"),
        "expected no trace output, got: {}",
        out
    );
}

#[tokio::test]
async fn yaml_off_plus_env_route_resolves_to_enabled() {
    use deterministic::{
        apply_enable_middleware, is_middleware_enabled, parse_enable_middleware_env,
        BackendAppConfig, MiddlewareEntry,
    };

    let mut cfg = BackendAppConfig {
        name: "generated-app".to_string(),
        middleware: vec![MiddlewareEntry {
            name: "traceRoute".to_string(),
            r#type: "route".to_string(),
            enabled: false,
            apply_routes: None,
            deny_routes: None,
        }],
        handlers: Vec::new(),
        statics: None,
    };
    apply_enable_middleware(
        &mut cfg.middleware,
        &parse_enable_middleware_env(Some("route")),
    );
    assert!(is_middleware_enabled(&cfg, "traceRoute"));
}

#[tokio::test]
async fn yaml_off_no_env_resolves_to_disabled() {
    use deterministic::{
        apply_enable_middleware, is_middleware_enabled, parse_enable_middleware_env,
        BackendAppConfig, MiddlewareEntry,
    };

    let mut cfg = BackendAppConfig {
        name: "generated-app".to_string(),
        middleware: vec![MiddlewareEntry {
            name: "traceRoute".to_string(),
            r#type: "route".to_string(),
            enabled: false,
            apply_routes: None,
            deny_routes: None,
        }],
        handlers: Vec::new(),
        statics: None,
    };
    apply_enable_middleware(&mut cfg.middleware, &parse_enable_middleware_env(None));
    assert!(!is_middleware_enabled(&cfg, "traceRoute"));
}

#[tokio::test]
async fn yaml_on_resolves_to_enabled_without_env() {
    use deterministic::{
        apply_enable_middleware, is_middleware_enabled, parse_enable_middleware_env,
        BackendAppConfig, MiddlewareEntry,
    };

    let mut cfg = BackendAppConfig {
        name: "generated-app".to_string(),
        middleware: vec![MiddlewareEntry {
            name: "traceRoute".to_string(),
            r#type: "route".to_string(),
            enabled: true,
            apply_routes: None,
            deny_routes: None,
        }],
        handlers: Vec::new(),
        statics: None,
    };
    apply_enable_middleware(&mut cfg.middleware, &parse_enable_middleware_env(None));
    assert!(is_middleware_enabled(&cfg, "traceRoute"));
}
