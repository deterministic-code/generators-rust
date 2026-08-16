use deterministic::middleware::{
    DataSourceConsoleLoggerMiddleware, ServiceConsoleLoggerMiddleware,
};
use deterministic::repositories::sqlite::{
    SqliteCrudRepository, SqliteDatasource, SqliteDatasourceOptions,
};
use deterministic::DataSourceMiddleware;
use deterministic::{CrudRepository, Datasource, RepositoryError, RowMap};
use serde_json::json;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

struct VecWriter(Arc<Mutex<Vec<u8>>>);

impl Write for VecWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn capture_writer() -> (Arc<Mutex<Vec<u8>>>, Box<dyn Write + Send>) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer: Box<dyn Write + Send> = Box::new(VecWriter(buf.clone()));
    (buf, writer)
}

fn captured(buf: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

async fn open_in_memory() -> Arc<SqliteDatasource> {
    let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
    ds.open().await.expect("open sqlite");
    Arc::new(ds)
}

async fn create_widgets_table(ds: &Arc<SqliteDatasource>) {
    ds.query(
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        &[],
    )
    .await
    .expect("create widgets table");
}

#[tokio::test]
async fn datasource_console_logger_emits_start_and_finish_for_crud_query() {
    let ds = open_in_memory().await;
    create_widgets_table(&ds).await;

    let (buf, writer) = capture_writer();
    let logger: Arc<dyn DataSourceMiddleware> =
        Arc::new(DataSourceConsoleLoggerMiddleware::with_writer(writer));

    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "widgets")
        .expect("repo")
        .with_middlewares(vec![logger]);

    let mut row = RowMap::new();
    row.insert("name".to_string(), json!("alpha"));
    let inserted = repo.add(row).await.expect("insert");
    let id = inserted
        .get("id")
        .and_then(|v| v.as_i64())
        .expect("inserted id");
    let _ = repo.find(&serde_json::Value::from(id)).await.expect("find");
    let _ = repo.find_all().await.expect("find_all");

    let out = captured(&buf);
    let starts = out.matches("[datasource][Start]-").count();
    let finishes = out.matches("[datasource][Finish]-").count();
    assert!(
        starts >= 3,
        "expected ≥3 datasource Start lines, got {}\n{}",
        starts,
        out
    );
    assert!(
        finishes >= 3,
        "expected ≥3 datasource Finish lines, got {}\n{}",
        finishes,
        out
    );
    assert!(out.contains("INSERT INTO"), "{}", out);
    assert!(out.contains("SELECT"), "{}", out);
}

#[tokio::test]
async fn datasource_console_logger_emits_error_line_on_failing_query() {
    let ds = open_in_memory().await;
    let (buf, writer) = capture_writer();
    let logger: Arc<dyn DataSourceMiddleware> =
        Arc::new(DataSourceConsoleLoggerMiddleware::with_writer(writer));

    let repo = SqliteCrudRepository::new(Arc::clone(&ds), "does_not_exist")
        .expect("repo")
        .with_middlewares(vec![logger]);

    let err = repo
        .find_all()
        .await
        .expect_err("missing table should error");
    let _ = err;

    let out = captured(&buf);
    assert!(
        out.contains("[datasource][Start]-"),
        "expected start line, got {}",
        out
    );
    assert!(
        out.contains("[datasource][Error]-"),
        "expected error line, got {}",
        out
    );
}

#[tokio::test]
async fn service_console_logger_emits_service_start_finish_around_business_call() {
    let (buf, writer) = capture_writer();
    let logger = ServiceConsoleLoggerMiddleware::with_writer(writer);

    async fn business_call() -> Result<i64, RepositoryError> {
        Ok(42)
    }

    let start = std::time::Instant::now();
    logger.before_call("WidgetService", "find_all");
    let result = business_call().await;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let err_msg = match &result {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
    logger.after_call("WidgetService", "find_all", elapsed_ms, err_msg.as_deref());

    let out = captured(&buf);
    assert!(
        out.contains("[service][Start]-[WidgetService.find_all]"),
        "{}",
        out
    );
    assert!(
        out.contains("[service][Finish]-[WidgetService.find_all]"),
        "{}",
        out
    );
}

#[tokio::test]
async fn service_console_logger_emits_service_error_when_business_call_fails() {
    let (buf, writer) = capture_writer();
    let logger = ServiceConsoleLoggerMiddleware::with_writer(writer);

    let result: Result<(), RepositoryError> = Err(RepositoryError::Other("boom".to_string()));

    logger.before_call("WidgetService", "delete");
    let err_msg = match &result {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
    logger.after_call("WidgetService", "delete", 5.0, err_msg.as_deref());

    let out = captured(&buf);
    assert!(
        out.contains("[service][Error]-[WidgetService.delete]"),
        "{}",
        out
    );
    assert!(out.contains("boom"), "{}", out);
}
