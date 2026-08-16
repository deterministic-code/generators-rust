use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use deterministic::{build_app, RunConfig};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_dir() -> PathBuf {
    let pid = std::process::id();
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("det-health-override-{}-{}", pid, n));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write(dir: &PathBuf, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

fn config_for(dir: PathBuf) -> RunConfig {
    RunConfig {
        deterministic_dir: dir,
        database_url: "sqlite::memory:".to_string(),
        port: 0,
        custom_services: deterministic::CustomServices::new(),
        route_composer: None,
    }
}

fn minimal_sample_with_routes(dir: &PathBuf, routes_yaml: &str, services_yaml: &str) {
    write(dir, "datasource_types.yaml", "types: []\n");
    write(dir, "routes.yaml", routes_yaml);
    write(dir, "services.yaml", services_yaml);
    write(dir, "view_types.yaml", "types: []\n");
    write(
        dir,
        "settings.yaml",
        "settings:\n  datasource:\n    pluralize_datatable_names: true\n",
    );
}

#[tokio::test]
async fn default_health_router_mounts_when_routes_yaml_omits_it() {
    let dir = tmp_dir();
    minimal_sample_with_routes(&dir, "routes: []\n", "services: []\n");
    let app = build_app(&config_for(dir)).await.unwrap();

    let res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body, json!({ "status": "ok" }));
}

// When routes.yaml names HealthCheckService but services.yaml never registers it (TS-only impl), the builtin must still serve /api/health or liveness probes 404.
#[tokio::test]
async fn builtin_serves_health_when_declared_service_is_not_registered() {
    let dir = tmp_dir();
    let routes_yaml = concat!(
        "routes:\n",
        "  - getHealth:\n",
        "      path: /api/health\n",
        "      method: GET\n",
        "      service: HealthCheckService\n",
        "      serviceMethod: check\n",
    );
    // services.yaml is empty — HealthCheckService isn't registered in Rust.
    minimal_sample_with_routes(&dir, routes_yaml, "services: []\n");
    let app = build_app(&config_for(dir)).await.unwrap();

    assert!(
        app.skipped_routes.iter().any(|r| r == "getHealth"),
        "expected getHealth to be skipped (no service registered), got skipped={:?} registered={:?}",
        app.skipped_routes,
        app.registered_routes,
    );

    let res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body, json!({ "status": "ok" }));
}

// A declared service with no runtime implementation must hard-fail at boot with an actionable error, never a null-returning stub masking as 200/null.
#[tokio::test]
async fn declared_but_unimplemented_service_errors_at_boot_with_actionable_message() {
    let dir = tmp_dir();
    let routes_yaml = concat!(
        "routes:\n",
        "  - getHealth:\n",
        "      path: /api/health\n",
        "      method: GET\n",
        "      service: HealthCheckService\n",
        "      serviceMethod: check\n",
    );
    // HealthCheckService has no module path and no matching entity, so the runtime has nothing to register.
    let services_yaml = concat!("services:\n", "  - name: HealthCheckService\n",);
    minimal_sample_with_routes(&dir, routes_yaml, services_yaml);

    let result = build_app(&config_for(dir)).await;
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected build_app to error; it succeeded"),
    };
    assert!(
        err.contains("HealthCheckService"),
        "error must name the missing service so the operator knows what to fix; got: {}",
        err,
    );
    assert!(
        err.contains("services.yaml"),
        "error must reference services.yaml so the operator finds the declaration; got: {}",
        err,
    );
}
