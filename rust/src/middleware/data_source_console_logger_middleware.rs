use std::io::{self, Write};
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::RepositoryError;
use crate::repositories::datasource_middleware::{
    DataSourceMiddleware, DatabaseBackend, RewrittenQuery,
};
use crate::trace::{format_elapsed_ms, format_trace_line, TracePhase, TraceTier};
use crate::util::now_iso;

const SQL_MAX_CHARS: usize = 1024;

pub struct DataSourceConsoleLoggerMiddleware {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl DataSourceConsoleLoggerMiddleware {
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

impl Default for DataSourceConsoleLoggerMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

fn render_sql(query: &str) -> String {
    let mut collapsed = String::with_capacity(query.len());
    let mut in_space = false;
    for c in query.chars() {
        if c.is_whitespace() {
            if !in_space && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            in_space = true;
        } else {
            collapsed.push(if c == ']' { ')' } else { c });
            in_space = false;
        }
    }
    if collapsed.ends_with(' ') {
        collapsed.pop();
    }
    if collapsed.chars().count() <= SQL_MAX_CHARS {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(SQL_MAX_CHARS - 1).collect();
    format!("{}…", truncated)
}

#[async_trait]
impl DataSourceMiddleware for DataSourceConsoleLoggerMiddleware {
    async fn before_query(
        &self,
        _provider: DatabaseBackend,
        query: &str,
        _params: Option<&[Value]>,
    ) -> Option<RewrittenQuery> {
        let sql = render_sql(query);
        let line = format_trace_line(
            &now_iso(),
            TraceTier::Datasource,
            TracePhase::Start,
            &sql,
            None,
        );
        self.write_line(&line);
        None
    }

    async fn after_query(
        &self,
        _provider: DatabaseBackend,
        query: &str,
        _params: Option<&[Value]>,
        _results: &[Value],
        elapsed_ms: f64,
        error: Option<&RepositoryError>,
    ) {
        let sql = render_sql(query);
        let elapsed = format_elapsed_ms(elapsed_ms);
        match error {
            Some(err) => {
                let suffix = format!("{} {}", err, elapsed);
                let line = format_trace_line(
                    &now_iso(),
                    TraceTier::Datasource,
                    TracePhase::Error,
                    &sql,
                    Some(&suffix),
                );
                self.write_line(&line);
            }
            None => {
                let line = format_trace_line(
                    &now_iso(),
                    TraceTier::Datasource,
                    TracePhase::Finish,
                    &sql,
                    Some(&elapsed),
                );
                self.write_line(&line);
            }
        }
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

    #[tokio::test]
    async fn before_query_emits_datasource_start_line_with_rendered_sql() {
        let (buf, writer) = buffer_pair();
        let logger = DataSourceConsoleLoggerMiddleware::with_writer(writer);
        let rewrite = logger
            .before_query(DatabaseBackend::Sqlite, "SELECT * FROM users", None)
            .await;
        assert!(rewrite.is_none(), "console logger must not rewrite queries");
        let out = captured(&buf);
        assert!(
            out.contains("[datasource][Start]-[SELECT * FROM users]"),
            "{}",
            out
        );
    }

    #[tokio::test]
    async fn after_query_no_error_emits_datasource_finish_with_elapsed() {
        let (buf, writer) = buffer_pair();
        let logger = DataSourceConsoleLoggerMiddleware::with_writer(writer);
        logger
            .after_query(DatabaseBackend::Sqlite, "SELECT 1", None, &[], 5.0, None)
            .await;
        let out = captured(&buf);
        assert!(
            out.contains("[datasource][Finish]-[SELECT 1] 5ms"),
            "{}",
            out
        );
    }

    #[tokio::test]
    async fn after_query_with_error_emits_datasource_error_with_message_and_elapsed() {
        let (buf, writer) = buffer_pair();
        let logger = DataSourceConsoleLoggerMiddleware::with_writer(writer);
        let err = RepositoryError::Other("connection refused".to_string());
        logger
            .after_query(
                DatabaseBackend::Postgres,
                "INSERT INTO t VALUES (1)",
                None,
                &[],
                2.0,
                Some(&err),
            )
            .await;
        let out = captured(&buf);
        assert!(
            out.contains("[datasource][Error]-[INSERT INTO t VALUES (1)]"),
            "{}",
            out
        );
        assert!(out.contains("connection refused"), "{}", out);
        assert!(out.contains(" 2ms"), "{}", out);
    }

    #[tokio::test]
    async fn render_sql_collapses_whitespace_and_replaces_closing_brackets() {
        let (buf, writer) = buffer_pair();
        let logger = DataSourceConsoleLoggerMiddleware::with_writer(writer);
        logger
            .before_query(
                DatabaseBackend::Sqlite,
                "SELECT   a,\n  b\tFROM   t WHERE x = ']' AND y = 1",
                None,
            )
            .await;
        let out = captured(&buf);
        assert!(
            out.contains("[datasource][Start]-[SELECT a, b FROM t WHERE x = ')' AND y = 1]"),
            "{}",
            out
        );
    }

    #[tokio::test]
    async fn render_sql_truncates_above_1024_chars_with_ellipsis() {
        let (buf, writer) = buffer_pair();
        let logger = DataSourceConsoleLoggerMiddleware::with_writer(writer);
        let huge = "x".repeat(2_000);
        let q = format!("SELECT '{}' AS pad", huge);
        logger.before_query(DatabaseBackend::Sqlite, &q, None).await;
        let out = captured(&buf);
        let line = out.lines().next().expect("at least one line");
        assert!(line.ends_with("…]"), "{}", line);
    }

    #[tokio::test]
    async fn trait_object_safe_via_arc_dyn() {
        let (_buf, writer) = buffer_pair();
        let logger: Arc<dyn DataSourceMiddleware> =
            Arc::new(DataSourceConsoleLoggerMiddleware::with_writer(writer));
        let _ = logger
            .before_query(DatabaseBackend::Sqlite, "SELECT 1", None)
            .await;
    }
}
