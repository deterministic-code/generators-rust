use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::run::{build_app, BuiltApp, OpenDatasource, RunConfig, RunError};

/// Running-server handle mirroring typescript/src/app/start.ts's StartHandle: the bound address, the route report, and a close() that drains in-flight requests and closes the datasource pool.
pub struct StartHandle {
    pub app_name: String,
    pub addr: SocketAddr,
    pub registered_routes: Vec<String>,
    pub skipped_routes: Vec<String>,
    datasource: Arc<OpenDatasource>,
    shutdown: oneshot::Sender<()>,
    server: JoinHandle<Result<(), RunError>>,
}

impl StartHandle {
    /// Gracefully stops the server (in-flight requests drain) and closes the datasource pool.
    pub async fn close(self) -> Result<(), RunError> {
        // send() only fails when the server task already exited; the join below surfaces that exit's real error.
        let _ = self.shutdown.send(());
        self.server
            .await
            .map_err(|join_err| RunError::Io(std::io::Error::other(join_err)))??;
        self.datasource.close_pool().await;
        Ok(())
    }
}

/// Mirrors typescript/src/app/start.ts: build the app, bind, serve in a background task with graceful shutdown, and return a closable handle.
pub async fn start(config: RunConfig) -> Result<StartHandle, RunError> {
    let app = build_app(&config).await?;
    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;
    let addr = listener.local_addr()?;
    println!(
        "{} listening on http://{} ({} routes, backend={:?})",
        app.app_name,
        addr,
        app.registered_routes.len(),
        app.datasource.dialect()
    );
    let BuiltApp {
        app_name,
        router,
        datasource,
        registered_routes,
        skipped_routes,
        ..
    } = app;
    let (shutdown, on_shutdown) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = on_shutdown.await;
            })
            .await
            .map_err(RunError::Io)
    });
    Ok(StartHandle {
        app_name,
        addr,
        registered_routes,
        skipped_routes,
        datasource,
        shutdown,
        server,
    })
}

/// Resolves on SIGINT (ctrl-c) or, on unix, SIGTERM — the same pair start.ts wires shutdown handlers for.
pub async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.expect("install ctrl-c handler"),
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("install ctrl-c handler");
    }
}
