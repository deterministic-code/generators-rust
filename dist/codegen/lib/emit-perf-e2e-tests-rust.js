const FILE_BODY = `use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct PerfStep {
    id: String,
    phase: String,
    op: String,
    path: String,
    #[serde(default)]
    body: Option<HashMap<String, Value>>,
    #[serde(default)]
    capture: Option<HashMap<String, String>>,
    #[serde(default)]
    if_match: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PerfPlan {
    #[allow(dead_code)]
    #[serde(default)]
    project_name: Option<String>,
    steps: Vec<PerfStep>,
}

#[derive(Default, Clone)]
struct RunCtx {
    ids: HashMap<String, Vec<Value>>,
    lookups: HashMap<String, LookupEntry>,
    i: usize,
}

#[derive(Default, Clone)]
struct LookupEntry {
    ids: Vec<Value>,
    first_id: Option<Value>,
    first_name: Option<Value>,
}

// Static counter guarantees uniqueness within the same nanosecond; SplitMix-style scramble obscures the counter so suffixes look random across calls and threads.
static RAND_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

fn rand_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_nanos() as u64;
    let count = RAND_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hash = nanos ^ count;
    hash = hash.wrapping_mul(0x517c_c1b7_2722_0955);
    hash ^= hash >> 33;
    format!("{:016x}", hash)
}

async fn find_deterministic_dir() -> PathBuf {
    if let Ok(p) = env::var("PERF_DETERMINISTIC_DIR") {
        return PathBuf::from(p);
    }
    let mut cur = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for _ in 0..6 {
        let candidate = cur.join("deterministic").join("performance-plan.yaml");
        if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
            return cur.join("deterministic");
        }
        if !cur.pop() {
            break;
        }
    }
    PathBuf::from("deterministic")
}

async fn resolve_plan_path() -> PathBuf {
    if let Ok(p) = env::var("PERF_PLAN_PATH") {
        return PathBuf::from(p);
    }
    find_deterministic_dir().await.join("performance-plan.yaml")
}

fn template_value(value: &str, ctx: &RunCtx) -> Result<Value, String> {
    let is_pure_substitution = is_pure_template(value);
    let replaced = replace_templates(value, ctx)?;
    if is_pure_substitution {
        if let Ok(n) = replaced.parse::<i64>() {
            return Ok(Value::from(n));
        }
    }
    Ok(Value::String(replaced))
}

fn is_pure_template(s: &str) -> bool {
    s.starts_with("\${") && s.ends_with("}") && s.matches('$').count() == 1
}

fn replace_templates(value: &str, ctx: &RunCtx) -> Result<String, String> {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            let close = value[i + 2..]
                .find('}')
                .ok_or_else(|| format!("unterminated template in {}", value))?;
            let expr = &value[i + 2..i + 2 + close];
            out.push_str(&resolve_expr(expr.trim(), ctx)?);
            i = i + 2 + close + 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

fn resolve_expr(expr: &str, ctx: &RunCtx) -> Result<String, String> {
    if expr == "randSuffix()" {
        return Ok(rand_suffix());
    }
    if expr == "i" {
        return Ok(ctx.i.to_string());
    }
    if let Some(name) = expr.strip_prefix("ids.").and_then(|t| t.strip_suffix("[i]")) {
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("unrecognized template expression {}", expr));
        }
        let pool = ctx
            .ids
            .get(name)
            .ok_or_else(|| format!("unresolved ids.{}[i] (no pool)", name))?;
        let v = pool
            .get(ctx.i)
            .ok_or_else(|| format!("unresolved ids.{}[i] (i={}, len={})", name, ctx.i, pool.len()))?;
        return Ok(value_to_string(v));
    }
    if let Some(name) = expr.strip_prefix("lookups.").and_then(|t| t.strip_suffix(".first_id")) {
        let lk = ctx
            .lookups
            .get(name)
            .ok_or_else(|| format!("unresolved lookups.{}.first_id", name))?;
        let v = lk
            .first_id
            .as_ref()
            .ok_or_else(|| format!("unresolved lookups.{}.first_id (null)", name))?;
        return Ok(value_to_string(v));
    }
    if let Some(name) = expr.strip_prefix("lookups.").and_then(|t| t.strip_suffix(".first_name")) {
        let lk = ctx
            .lookups
            .get(name)
            .ok_or_else(|| format!("unresolved lookups.{}.first_name", name))?;
        let v = lk
            .first_name
            .as_ref()
            .ok_or_else(|| format!("unresolved lookups.{}.first_name (null)", name))?;
        return Ok(value_to_string(v));
    }
    if let Some(rest) = expr.strip_prefix("lookups.") {
        if let Some(dot) = rest.find(".ids[") {
            let name = &rest[..dot];
            let close = rest
                .find(']')
                .ok_or_else(|| format!("malformed lookups.{}.ids[N]", name))?;
            let idx_str = &rest[dot + 5..close];
            let idx: usize = idx_str
                .parse()
                .map_err(|_| format!("non-numeric index in {}", expr))?;
            let lk = ctx
                .lookups
                .get(name)
                .ok_or_else(|| format!("unresolved lookups.{}.ids[{}]", name, idx))?;
            let v = lk
                .ids
                .get(idx)
                .ok_or_else(|| format!("unresolved lookups.{}.ids[{}]", name, idx))?;
            return Ok(value_to_string(v));
        }
    }
    Err(format!("unrecognized template expression {}", expr))
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn resolve_value_deep(value: &Value, ctx: &RunCtx) -> Result<Value, String> {
    match value {
        Value::String(s) => template_value(s, ctx),
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                out.push(resolve_value_deep(v, ctx)?);
            }
            Ok(Value::Array(out))
        }
        Value::Object(obj) => {
            let mut out = serde_json::Map::new();
            for (k, v) in obj {
                out.insert(k.clone(), resolve_value_deep(v, ctx)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn resolve_body(
    body: Option<&HashMap<String, Value>>,
    ctx: &RunCtx,
) -> Result<Option<Value>, String> {
    let Some(map) = body else {
        return Ok(None);
    };
    let mut out = serde_json::Map::new();
    for (k, v) in map {
        out.insert(k.clone(), resolve_value_deep(v, ctx)?);
    }
    Ok(Some(Value::Object(out)))
}

fn extract_by_json_path(body: &Value, path: &str) -> Result<Value, String> {
    if let Some(col) = path.strip_prefix("$.").and_then(simple_column) {
        if let Some(v) = body.get(col) {
            return Ok(v.clone());
        }
        if let Some(item_v) = body.get("item").and_then(|i| i.get(col)) {
            return Ok(item_v.clone());
        }
        return Ok(Value::Null);
    }
    if let Some(col) = path
        .strip_prefix("$.items[0].")
        .and_then(simple_column)
    {
        let items = body.get("items").and_then(|v| v.as_array());
        return Ok(items
            .and_then(|arr| arr.first().and_then(|it| it.get(col)).cloned())
            .unwrap_or(Value::Null));
    }
    if let Some(col) = path
        .strip_prefix("$.items[*].")
        .and_then(simple_column)
    {
        let items = body.get("items").and_then(|v| v.as_array());
        return Ok(Value::Array(
            items
                .map(|arr| {
                    arr.iter()
                        .map(|it| it.get(col).cloned().unwrap_or(Value::Null))
                        .collect()
                })
                .unwrap_or_default(),
        ));
    }
    Err(format!("unsupported JSONPath {}", path))
}

fn simple_column(rest: &str) -> Option<&str> {
    if rest.is_empty() {
        return None;
    }
    if !rest
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(rest)
}

fn apply_capture(
    capture: Option<&HashMap<String, String>>,
    body: &Value,
    ctx: &mut RunCtx,
) -> Result<(), String> {
    let Some(spec) = capture else {
        return Ok(());
    };
    for (target, json_path) in spec {
        let extracted = extract_by_json_path(body, json_path)?;
        if let Some(name) = target.strip_prefix("ids.").and_then(|t| t.strip_suffix("[i]")) {
            let pool = ctx.ids.entry(name.to_string()).or_default();
            while pool.len() <= ctx.i {
                pool.push(Value::Null);
            }
            pool[ctx.i] = extracted;
        } else if let Some(rest) = target.strip_prefix("lookups.") {
            if let Some((name, kind)) = rest.split_once('.') {
                let entry = ctx.lookups.entry(name.to_string()).or_default();
                match kind {
                    "ids" => {
                        entry.ids = extracted.as_array().cloned().unwrap_or_default();
                    }
                    "first_id" => {
                        entry.first_id = if extracted.is_null() {
                            None
                        } else {
                            Some(extracted)
                        };
                    }
                    "first_name" => {
                        entry.first_name = if extracted.is_null() {
                            None
                        } else {
                            Some(extracted)
                        };
                    }
                    _ => return Err(format!("unsupported capture target {}", target)),
                }
            }
        }
    }
    Ok(())
}

fn expected_status_for(op: &str) -> &'static [u16] {
    match op {
        "POST" => &[201],
        "GET" | "PUT" | "PATCH" => &[200],
        "DELETE" => &[200, 204],
        _ => &[200],
    }
}

async fn execute_step(
    client: &reqwest::Client,
    base_url: &str,
    step: &PerfStep,
    ctx: &mut RunCtx,
    timings: &mut HashMap<String, Vec<f64>>,
    record: bool,
) -> Result<(), String> {
    let resolved_path = template_value(&step.path, ctx)?;
    let path_str = match resolved_path {
        Value::String(s) => s,
        v => value_to_string(&v),
    };
    let body = resolve_body(step.body.as_ref(), ctx)?;
    let url = format!("{}{}", base_url, path_str);

    let method = reqwest::Method::from_bytes(step.op.as_bytes())
        .map_err(|e| format!("unsupported method {}: {}", step.op, e))?;
    let mut req = client.request(method, &url);
    if let Some(b) = &body {
        req = req
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(b).map_err(|e| format!("body serialize: {}", e))?);
    }
    if let Some(if_match_template) = &step.if_match {
        let resolved = template_value(if_match_template, ctx)?;
        let header_value = match resolved {
            Value::String(s) => s,
            v => value_to_string(&v),
        };
        if header_value.is_empty() || header_value == "null" {
            return Err(format!(
                "step {}: If-Match {} resolved to '{}' — the captured updated token is missing (empty version pool)",
                step.id, if_match_template, header_value
            ));
        }
        req = req.header("If-Match", header_value);
    }

    let t0 = Instant::now();
    let res = req
        .send()
        .await
        .map_err(|e| format!("send {} {}: {}", step.op, url, e))?;
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    if record {
        timings.entry(step.id.clone()).or_default().push(elapsed_ms);
    }

    let status = res.status().as_u16();
    let expected = expected_status_for(&step.op);
    if !expected.contains(&status) {
        let body_text = res.text().await.unwrap_or_default();
        let sent_json = body
            .as_ref()
            .map(|b| serde_json::to_string(b).unwrap_or_default())
            .unwrap_or_else(|| "<none>".to_string());
        let safe_body = body_text.chars().take(500).collect::<String>();
        let safe_sent = sent_json.chars().take(500).collect::<String>();
        return Err(format!(
            "step {} {} {} expected {:?} got {} body={} sent={}",
            step.id, step.op, url, expected, status, safe_body, safe_sent
        ));
    }

    if step.capture.is_some() {
        let parsed: Value = res.json().await.unwrap_or(Value::Null);
        apply_capture(step.capture.as_ref(), &parsed, ctx)?;
    }

    Ok(())
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    let idx = (p * n as f64).floor() as usize;
    let clamped = idx.min(n - 1);
    sorted[clamped]
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires running backend at PERF_BASE_URL — run via cargo test --test app_perf_client -- --ignored --nocapture"]
async fn perf_e2e_run_performance_plan() {
    let iterations: usize = env::var("PERF_ITERATIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    let warmup: usize = env::var("PERF_WARMUP_ITERATIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let base_url =
        env::var("PERF_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
    let dialect = env::var("PERF_DIALECT")
        .or_else(|_| env::var("DATABASE_BACKEND"))
        .unwrap_or_else(|_| "sqlite".to_string());
    let language = env::var("PERF_LANGUAGE").unwrap_or_else(|_| "rust".to_string());

    let plan_path = resolve_plan_path().await;
    let plan_text = tokio::fs::read_to_string(&plan_path)
        .await
        .unwrap_or_else(|e| panic!("read perf plan {}: {}", plan_path.display(), e));
    let plan: PerfPlan = serde_yaml::from_str(&plan_text)
        .unwrap_or_else(|e| panic!("parse perf plan {}: {}", plan_path.display(), e));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("build reqwest client");

    let mut ctx = RunCtx::default();
    let mut timings: HashMap<String, Vec<f64>> = HashMap::new();
    let mut step_op: HashMap<String, String> = HashMap::new();
    let mut step_path: HashMap<String, String> = HashMap::new();
    for s in &plan.steps {
        step_op.insert(s.id.clone(), s.op.clone());
        step_path.insert(s.id.clone(), s.path.clone());
    }

    let lookup_steps: Vec<&PerfStep> = plan.steps.iter().filter(|s| s.phase == "lookup").collect();
    let iter_steps: Vec<&PerfStep> = plan.steps.iter().filter(|s| s.phase == "iter").collect();
    let teardown_steps: Vec<&PerfStep> =
        plan.steps.iter().filter(|s| s.phase == "iter_teardown").collect();

    if warmup > 0 {
        for step in &lookup_steps {
            execute_step(&client, &base_url, step, &mut ctx, &mut timings, false)
                .await
                .unwrap_or_else(|e| panic!("warmup lookup: {}", e));
        }
        for w in 0..warmup {
            let mut warm_ctx = RunCtx {
                ids: HashMap::new(),
                lookups: ctx.lookups.clone(),
                i: w,
            };
            for step in &iter_steps {
                execute_step(&client, &base_url, step, &mut warm_ctx, &mut timings, false)
                    .await
                    .unwrap_or_else(|e| panic!("warmup iter {}: {}", w, e));
            }
            for step in &teardown_steps {
                execute_step(&client, &base_url, step, &mut warm_ctx, &mut timings, false)
                    .await
                    .unwrap_or_else(|e| panic!("warmup teardown {}: {}", w, e));
            }
        }
    }

    for step in &lookup_steps {
        execute_step(&client, &base_url, step, &mut ctx, &mut timings, true)
            .await
            .unwrap_or_else(|e| panic!("lookup: {}", e));
    }

    for iter in 0..iterations {
        ctx.i = iter;
        for step in &iter_steps {
            execute_step(&client, &base_url, step, &mut ctx, &mut timings, true)
                .await
                .unwrap_or_else(|e| panic!("iter {}: {}", iter, e));
        }
        for step in &teardown_steps {
            execute_step(&client, &base_url, step, &mut ctx, &mut timings, true)
                .await
                .unwrap_or_else(|e| panic!("teardown {}: {}", iter, e));
        }
    }

    let mut step_ids: Vec<&String> = timings.keys().collect();
    step_ids.sort();
    for id in step_ids {
        let samples = timings.get(id).unwrap();
        if samples.is_empty() {
            continue;
        }
        let mut sorted = samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p50 = percentile(&sorted, 0.50);
        let p95 = percentile(&sorted, 0.95);
        let max = sorted.last().copied().unwrap_or(0.0);
        let op = step_op.get(id).cloned().unwrap_or_else(|| "?".to_string());
        let path = step_path.get(id).cloned().unwrap_or_else(|| "?".to_string());
        let iso = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        println!(
            "[{}]-[perf-e2e][Finish]-[{} {}] step_id={} language={} dialect={} iterations={} p50={}ms p95={}ms max={}ms",
            iso,
            op,
            path,
            id,
            language,
            dialect,
            samples.len(),
            round3(p50),
            round3(p95),
            round3(max),
        );
    }
}
`;
export function emitPerfE2eTestRust() {
    return { path: "tests/app_perf_client.rs", content: FILE_BODY };
}
const PERF_E2E_BASENAME = "app_perf_client.rs";
/** The perf e2e client is a `tests/` integration binary, outside the crate's mod.rs graph — no rust module wiring. */
export const rustModuleWiring = false;
/** Self-describing catalog `perf_e2e_tests` (rust): the perf e2e client binary that replays performance-plan.yaml against a running backend. The `--output` already resolves to the crate's `tests` dir, so the file is placed by basename. */
export async function emit() {
    return {
        files: [
            { path: PERF_E2E_BASENAME, content: emitPerfE2eTestRust().content },
        ],
    };
}
