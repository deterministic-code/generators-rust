use std::path::PathBuf;

use deterministic::{start, RunConfig};

fn email_sample_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust crate has parent")
        .join("samples")
        .join("email-backend")
        .join("deterministic")
}

// Boots with no route composer, so only the default /api/health handler is mounted (no entity
// routes) — the point here is the transport lifecycle, not routing: start() serves over real TCP,
// health answers, and close() drains the connection and frees the port.
#[tokio::test(flavor = "multi_thread")]
async fn start_serves_over_tcp_and_close_drains_and_frees_the_port() {
    let deterministic_dir = email_sample_dir();
    if !deterministic_dir.is_dir() {
        eprintln!("skipping: sample dir {:?} not found", deterministic_dir);
        return;
    }
    let config = RunConfig {
        deterministic_dir,
        database_url: "sqlite::memory:".to_string(),
        port: 0,
        custom_services: deterministic::CustomServices::new(),
        route_composer: None,
    };
    let handle = start(config).await.expect("start");
    // The sample ships no backend-app.yaml, so the loader default drives the startup/shutdown log name.
    assert_eq!(handle.app_name, "generated-app");

    let addr = handle.addr;
    let body: serde_json::Value = reqwest::get(format!("http://{addr}/api/health"))
        .await
        .expect("health request")
        .json()
        .await
        .expect("health json");
    assert_eq!(body["status"], "ok");

    handle.close().await.expect("close");

    let after = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client")
        .get(format!("http://{addr}/api/health"))
        .send()
        .await;
    assert!(
        after.is_err(),
        "closed server must refuse connections, got: {after:?}"
    );
}
