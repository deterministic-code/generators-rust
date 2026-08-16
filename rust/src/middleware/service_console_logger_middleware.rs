use std::io::{self, Write};
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;

use crate::services::{ServiceError, ServiceMiddleware};
use crate::trace::{format_elapsed_ms, format_trace_line, TracePhase, TraceTier};
use crate::util::now_iso;

pub struct ServiceConsoleLoggerMiddleware {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl ServiceConsoleLoggerMiddleware {
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

    pub fn before_call(&self, service_name: &str, method_name: &str) {
        let method = format!("{}.{}", service_name, method_name);
        let line = format_trace_line(
            &now_iso(),
            TraceTier::Service,
            TracePhase::Start,
            &method,
            None,
        );
        self.write_line(&line);
    }

    pub fn after_call(
        &self,
        service_name: &str,
        method_name: &str,
        elapsed_ms: f64,
        error_message: Option<&str>,
    ) {
        let method = format!("{}.{}", service_name, method_name);
        let elapsed = format_elapsed_ms(elapsed_ms);
        match error_message {
            Some(msg) => {
                let suffix = format!("{} {}", msg, elapsed);
                let line = format_trace_line(
                    &now_iso(),
                    TraceTier::Service,
                    TracePhase::Error,
                    &method,
                    Some(&suffix),
                );
                self.write_line(&line);
            }
            None => {
                let line = format_trace_line(
                    &now_iso(),
                    TraceTier::Service,
                    TracePhase::Finish,
                    &method,
                    Some(&elapsed),
                );
                self.write_line(&line);
            }
        }
    }
}

impl Default for ServiceConsoleLoggerMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceMiddleware for ServiceConsoleLoggerMiddleware {
    async fn before_call(&self, service_name: &str, method_name: &str, _args: &Value) {
        self.before_call(service_name, method_name);
    }

    async fn after_call(
        &self,
        service_name: &str,
        method_name: &str,
        _args: &Value,
        result: &Result<Value, ServiceError>,
        elapsed_ms: f64,
    ) {
        let error_message = result.as_ref().err().map(|e| e.to_string());
        self.after_call(
            service_name,
            method_name,
            elapsed_ms,
            error_message.as_deref(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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

    #[test]
    fn before_call_emits_service_start_line_with_service_dot_method() {
        let (buf, writer) = buffer_pair();
        let logger = ServiceConsoleLoggerMiddleware::with_writer(writer);
        logger.before_call("UsersService", "findById");
        let out = captured(&buf);
        assert!(
            out.contains("[service][Start]-[UsersService.findById]"),
            "{}",
            out
        );
    }

    #[test]
    fn after_call_with_no_error_emits_service_finish_with_elapsed() {
        let (buf, writer) = buffer_pair();
        let logger = ServiceConsoleLoggerMiddleware::with_writer(writer);
        logger.after_call("UsersService", "findById", 12.0, None);
        let out = captured(&buf);
        assert!(
            out.contains("[service][Finish]-[UsersService.findById] 12ms"),
            "{}",
            out
        );
    }

    #[test]
    fn after_call_with_error_emits_service_error_with_message_and_elapsed() {
        let (buf, writer) = buffer_pair();
        let logger = ServiceConsoleLoggerMiddleware::with_writer(writer);
        logger.after_call("UsersService", "findById", 7.0, Some("boom"));
        let out = captured(&buf);
        assert!(
            out.contains("[service][Error]-[UsersService.findById] boom 7ms"),
            "{}",
            out
        );
    }

    #[test]
    fn after_call_with_zero_elapsed_emits_0ns() {
        let (buf, writer) = buffer_pair();
        let logger = ServiceConsoleLoggerMiddleware::with_writer(writer);
        logger.after_call("X", "y", 0.0, None);
        let out = captured(&buf);
        assert!(out.contains("[service][Finish]-[X.y] 0ns"), "{}", out);
    }

    #[test]
    fn writes_one_line_per_call_terminated_by_newline() {
        let (buf, writer) = buffer_pair();
        let logger = ServiceConsoleLoggerMiddleware::with_writer(writer);
        logger.before_call("S", "m");
        logger.after_call("S", "m", 1.0, None);
        let out = captured(&buf);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{}", out);
        assert!(lines[0].contains("[service][Start]"));
        assert!(lines[1].contains("[service][Finish]"));
    }
}
