use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::trace::{format_elapsed_ms, format_trace_line, TracePhase, TraceTier};
use crate::util::now_iso;

pub struct TraceRouteMiddleware {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl TraceRouteMiddleware {
    pub fn new() -> Self {
        Self::with_writer(Box::new(io::stdout()))
    }

    pub fn with_writer(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer: Mutex::new(writer),
        }
    }

    fn write_line(&self, line: &str) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = writeln!(w, "{}", line);
        }
    }
}

impl Default for TraceRouteMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn trace_route_layer(
    State(logger): State<Arc<TraceRouteMiddleware>>,
    req: Request,
    next: Next,
) -> Response {
    let method_label = format!("{} {}", req.method(), req.uri().path());
    logger.write_line(&format_trace_line(
        &now_iso(),
        TraceTier::Route,
        TracePhase::Start,
        &method_label,
        None,
    ));

    let start = Instant::now();
    let response = next.run(req).await;
    let elapsed = format_elapsed_ms(start.elapsed().as_secs_f64() * 1000.0);
    let status = response.status().as_u16();

    logger.write_line(&format_trace_line(
        &now_iso(),
        TraceTier::Route,
        TracePhase::Finish,
        &method_label,
        Some(&format!("{} {}", status, elapsed)),
    ));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use std::sync::Arc;
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

    fn build_router_with_layer(
        logger: Arc<TraceRouteMiddleware>,
        handler: axum::routing::MethodRouter,
    ) -> Router {
        Router::new()
            .route("/x", handler)
            .layer(axum::middleware::from_fn_with_state(
                logger,
                trace_route_layer,
            ))
    }

    #[tokio::test]
    async fn trace_route_layer_emits_start_with_method_and_path() {
        let (buf, writer) = buffer_pair();
        let logger = Arc::new(TraceRouteMiddleware::with_writer(writer));
        let app = build_router_with_layer(logger, get(|| async { "ok" }));
        let req = HttpRequest::builder()
            .method("GET")
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let _ = res.into_body().collect().await.unwrap();
        let out = captured(&buf);
        assert!(out.contains("[route][Start]-[GET /x]"), "{}", out);
    }

    #[tokio::test]
    async fn trace_route_layer_emits_finish_with_status_and_elapsed() {
        let (buf, writer) = buffer_pair();
        let logger = Arc::new(TraceRouteMiddleware::with_writer(writer));
        let app = build_router_with_layer(logger, get(|| async { "ok" }));
        let req = HttpRequest::builder()
            .method("GET")
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let _ = res.into_body().collect().await.unwrap();
        let out = captured(&buf);
        assert!(out.contains("[route][Finish]-[GET /x] 200 "), "{}", out);
    }

    #[tokio::test]
    async fn trace_route_layer_emits_finish_with_5xx_status() {
        let (buf, writer) = buffer_pair();
        let logger = Arc::new(TraceRouteMiddleware::with_writer(writer));
        let app =
            build_router_with_layer(logger, get(|| async { StatusCode::INTERNAL_SERVER_ERROR }));
        let req = HttpRequest::builder()
            .method("GET")
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let _ = res.into_body().collect().await.unwrap();
        let out = captured(&buf);
        assert!(out.contains("[route][Finish]-[GET /x] 500 "), "{}", out);
    }

    #[tokio::test]
    async fn trace_route_layer_orders_start_before_finish() {
        let (buf, writer) = buffer_pair();
        let logger = Arc::new(TraceRouteMiddleware::with_writer(writer));
        let app = build_router_with_layer(logger, get(|| async { "ok" }));
        let req = HttpRequest::builder()
            .method("GET")
            .uri("/x")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let _ = res.into_body().collect().await.unwrap();
        let out = captured(&buf);
        let start_idx = out.find("[route][Start]").expect("start present");
        let finish_idx = out.find("[route][Finish]").expect("finish present");
        assert!(start_idx < finish_idx, "{}", out);
    }
}
