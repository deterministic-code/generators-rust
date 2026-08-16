// Default: applies ALL pending migrations in sequence, each in its own transaction. Pass --one to apply only the next pending migration (legacy one-shot behavior; useful when wrapping in an external loop or when you want to inspect state between steps).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

// === BEGIN DIALECT_USE_IMPORTS_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
use sqlx::{Executor, MySqlPool, PgPool, Row, SqlitePool};
// === END DIALECT_USE_IMPORTS_RUST ===

const HELP_TEXT: &str = r#"Usage: migrate-up --provider <sqlite|postgres|mysql> --connection <url> [--migrations-path <dir>] [--migrations-root <dir>] [--one]

Applies ALL pending migrations in sequence, each in its own transaction.

  --provider          Database dialect. Required.
  --connection        Connection string. Required.
  --migrations-path   Full path to the migrations directory (e.g. sql/sqlite/migrations).
  --migrations-root   Root prefix; the script then looks at <migrations-root>/<dialect>/migrations.
                      Defaults to 'sql'. Ignored when --migrations-path is also set.
  --one               Apply only the next pending migration, then exit. Default is to apply all pending migrations.

Examples:
  # sqlite — bare path or sqlite:// URL; both create a file on disk.
  migrate-up --provider sqlite --connection ./app.sqlite
  migrate-up --provider sqlite --connection sqlite:///absolute/path/app.sqlite

  # postgres — URL form (libpq keyword form is accepted by sqlx too).
  migrate-up --provider postgres --connection postgresql://user:pass@host:5432/dbname

  # mysql — URL form.
  migrate-up --provider mysql --connection mysql://user:pass@host:3306/dbname

  # custom migrations layout (default looks under sql/<dialect>/migrations):
  migrate-up --provider sqlite --connection ./app.sqlite --migrations-path ./db/changes/sqlite
  migrate-up --provider sqlite --connection ./app.sqlite --migrations-root ./db/changes

  # one-shot: apply the next pending migration only, then exit.
  migrate-up --provider sqlite --connection ./app.sqlite --one

Note: when --connection is omitted, ./.env is loaded (if present) and the
runner falls back to per-dialect env vars — sqlite: SQLITE_PATH, DB_PATH;
postgres: PG_CONNECTION_STRING, DATABASE_URL; mysql: MYSQL_URL, DATABASE_URL.
The typescript and csharp runners honor the same fallback.
"#;

struct Args {
    provider: String,
    migrate_path: String,
    connection: String,
    one: bool,
}

// Mirrors the TypeScript runner's per-dialect fallback so both lanes accept the same environment.
fn connection_env_vars(provider: &str) -> &'static [&'static str] {
    match provider {
        "sqlite" => &["SQLITE_PATH", "DB_PATH"],
        "postgres" => &["PG_CONNECTION_STRING", "DATABASE_URL"],
        "mysql" => &["MYSQL_URL", "DATABASE_URL"],
        _ => &[],
    }
}

fn connection_from_env(provider: &str) -> Option<String> {
    connection_env_vars(provider)
        .iter()
        .find_map(|name| env::var(name).ok().filter(|v| !v.is_empty()))
}

fn parse_args() -> Result<Args, String> {
    let mut provider: Option<String> = None;
    let mut migrate_path: Option<String> = None;
    let mut migrate_root: Option<String> = None;
    let mut connection: Option<String> = None;
    let mut one = false;
    let mut iter = env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--provider" => provider = iter.next(),
            // --migrate-path / --migrate-root are silent aliases retained for backward compat.
            "--migrations-path" => migrate_path = iter.next(),
            "--migrate-path" => migrate_path = iter.next(),
            "--migrations-root" => migrate_root = iter.next(),
            "--migrate-root" => migrate_root = iter.next(),
            "--connection" => connection = iter.next(),
            "--one" => one = true,
            "-h" | "--help" => {
                eprint!("{}", HELP_TEXT);
                process::exit(0);
            }
            other => return Err(format!("unknown arg: {}", other)),
        }
    }
    let Some(provider) = provider else {
        return Err("missing --provider — pass --provider <sqlite|postgres|mysql>. Run with --help for examples.".to_string());
    };
    let migrate_path = migrate_path.unwrap_or_else(|| {
        let root = migrate_root.unwrap_or_else(|| "sql".to_string());
        format!("{}/{}/migrations", root, provider)
    });
    let connection = match connection.or_else(|| connection_from_env(&provider)) {
        Some(c) => c,
        None => {
            return Err(format!(
                "missing --connection — pass --connection <url> (e.g. ./app.sqlite for sqlite) or set one of: {}. Run with --help for examples.",
                connection_env_vars(&provider).join(", ")
            ))
        }
    };
    Ok(Args {
        provider,
        migrate_path,
        connection,
        one,
    })
}

fn discover_up_files(
    migrate_path: &str,
) -> Result<Vec<(String, PathBuf)>, Box<dyn std::error::Error>> {
    let dir = PathBuf::from(migrate_path);
    if !dir.is_dir() {
        return Err(format!("migrate-path is not a directory: {}", migrate_path).into());
    }
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) if n.ends_with("_up.sql") => n.to_string(),
            _ => continue,
        };
        out.push((name, path));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn checksum_hex(sql: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in sql.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

// Block-aware: BEGIN/END (case-insensitive) inside a CREATE TRIGGER body is tracked so the inner UPDATE's `;` does not terminate the outer statement (sqlite error code 1 "incomplete input" otherwise).
// A whole-line `GO` is a batch separator (mysql/sqlserver stored-procedure DDL): it flushes the current statement and is dropped, so `DROP …; GO CREATE PROCEDURE … END; GO` splits into clean per-procedure batches.
fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_dollar_tag: Option<String> = None;
    let mut block_depth: i32 = 0;
    let mut word = String::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if in_dollar_tag.is_none()
            && !in_line_comment
            && !in_block_comment
            && !in_single_quote
            && !in_double_quote
            && (i == 0 || chars[i - 1] == '\n')
        {
            let mut j = i;
            while j < chars.len() && chars[j] != '\n' {
                j += 1;
            }
            if chars[i..j]
                .iter()
                .collect::<String>()
                .trim()
                .eq_ignore_ascii_case("GO")
            {
                let s = buf.trim().to_string();
                if !s.is_empty() {
                    out.push(s);
                }
                buf.clear();
                block_depth = 0;
                word.clear();
                i = if j < chars.len() { j + 1 } else { j };
                continue;
            }
        }
        if !in_line_comment
            && !in_block_comment
            && in_dollar_tag.is_none()
            && !in_single_quote
            && !in_double_quote
        {
            if c == '-' && next == Some('-') {
                in_line_comment = true;
                buf.push(c);
                i += 1;
                continue;
            }
            if c == '/' && next == Some('*') {
                in_block_comment = true;
                buf.push(c);
                i += 1;
                continue;
            }
            if c == '$' {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '$' {
                    j += 1;
                }
                if j < chars.len() {
                    let tag: String = chars[i..=j].iter().collect();
                    in_dollar_tag = Some(tag);
                    for k in i..=j {
                        buf.push(chars[k]);
                    }
                    i = j + 1;
                    continue;
                }
            }
        }
        if in_line_comment {
            buf.push(c);
            if c == '\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            buf.push(c);
            if c == '*' && next == Some('/') {
                buf.push('/');
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(tag) = &in_dollar_tag {
            buf.push(c);
            if c == '$' {
                let end = i + tag.len();
                if end <= chars.len() {
                    let candidate: String = chars[i..end].iter().collect();
                    if &candidate == tag {
                        for k in (i + 1)..end {
                            buf.push(chars[k]);
                        }
                        in_dollar_tag = None;
                        i = end;
                        continue;
                    }
                }
            }
            i += 1;
            continue;
        }
        if in_single_quote {
            buf.push(c);
            if c == '\'' {
                in_single_quote = false;
            }
            i += 1;
            continue;
        }
        if in_double_quote {
            buf.push(c);
            if c == '"' {
                in_double_quote = false;
            }
            i += 1;
            continue;
        }
        if c == '\'' {
            in_single_quote = true;
            buf.push(c);
            i += 1;
            continue;
        }
        if c == '"' {
            in_double_quote = true;
            buf.push(c);
            i += 1;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            word.push(c);
            buf.push(c);
            i += 1;
            continue;
        }
        if !word.is_empty() {
            let upper = word.to_ascii_uppercase();
            if upper == "BEGIN" {
                block_depth += 1;
            } else if upper == "END" && block_depth > 0 {
                block_depth -= 1;
            }
            word.clear();
        }
        if c == ';' && block_depth == 0 {
            buf.push(c);
            let s = buf.trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
            buf.clear();
            i += 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    let tail = buf.trim().to_string();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

// === BEGIN DIALECT_SQLITE_URL_HELPER_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
fn sqlite_url(connection: &str) -> String {
    if connection.starts_with("sqlite:") {
        connection.to_string()
    } else {
        format!("sqlite://{}?mode=rwc", connection)
    }
}
// === END DIALECT_SQLITE_URL_HELPER_RUST ===

// Returns the filesystem path for a sqlite connection string, or None for :memory: (which is always ephemeral and should skip the existence check).
fn sqlite_filesystem_path(connection: &str) -> Option<String> {
    let mut s = connection.trim().to_string();
    if let Some(eq) = s.find('=') {
        let key = s[..eq].trim().to_ascii_lowercase();
        if key.replace(' ', "") == "datasource" {
            s = s[eq + 1..].trim().to_string();
        }
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("sqlite://") {
        s = s["sqlite://".len()..].to_string();
    } else if lower.starts_with("sqlite:") {
        s = s["sqlite:".len()..].to_string();
    } else if lower.starts_with("file:") {
        s = s["file:".len()..].to_string();
    }
    if s == ":memory:" || s.is_empty() {
        return None;
    }
    Some(s)
}

// === BEGIN DIALECT_RUNNER_FNS_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
async fn run_sqlite(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePool::connect(&sqlite_url(&args.connection)).await?;
    let mut applied_set: std::collections::HashSet<String> =
        sqlx::query(r#"SELECT "name" FROM "migrates""#)
            .fetch_all(&pool)
            .await?
            .into_iter()
            .map(|r| r.get::<String, _>(0))
            .collect();

    let files = discover_up_files(&args.migrate_path)?;
    let mut applied_count = 0usize;
    loop {
        let next = files
            .iter()
            .find(|(n, _)| !applied_set.contains(n))
            .cloned();
        let (name, path) = match next {
            Some(t) => t,
            None => {
                if applied_count == 0 {
                    println!("No pending migrations.");
                } else {
                    println!("No more pending migrations.");
                }
                return Ok(());
            }
        };
        let sql = fs::read_to_string(&path)?;
        let sum = checksum_hex(&sql);

        let mut tx = pool.begin().await?;
        for stmt in split_statements(&sql) {
            tx.execute(stmt.as_str()).await?;
        }
        sqlx::query(r#"INSERT INTO "migrates" ("name", "checksum") VALUES (?, ?)"#)
            .bind(&name)
            .bind(&sum)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        println!("Applied: {}", name);
        applied_set.insert(name);
        applied_count += 1;
        if args.one {
            return Ok(());
        }
    }
}

async fn run_postgres(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = PgPool::connect(&args.connection).await?;
    let mut applied_set: std::collections::HashSet<String> =
        sqlx::query(r#"SELECT "name" FROM "migrates""#)
            .fetch_all(&pool)
            .await?
            .into_iter()
            .map(|r| r.get::<String, _>(0))
            .collect();

    let files = discover_up_files(&args.migrate_path)?;
    let mut applied_count = 0usize;
    loop {
        let next = files
            .iter()
            .find(|(n, _)| !applied_set.contains(n))
            .cloned();
        let (name, path) = match next {
            Some(t) => t,
            None => {
                if applied_count == 0 {
                    println!("No pending migrations.");
                } else {
                    println!("No more pending migrations.");
                }
                return Ok(());
            }
        };
        let sql = fs::read_to_string(&path)?;
        let sum = checksum_hex(&sql);

        let mut tx = pool.begin().await?;
        for stmt in split_statements(&sql) {
            tx.execute(stmt.as_str()).await?;
        }
        sqlx::query(r#"INSERT INTO "migrates" ("name", "checksum") VALUES ($1, $2)"#)
            .bind(&name)
            .bind(&sum)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        println!("Applied: {}", name);
        applied_set.insert(name);
        applied_count += 1;
        if args.one {
            return Ok(());
        }
    }
}

async fn run_mysql(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = MySqlPool::connect(&args.connection).await?;
    let mut applied_set: std::collections::HashSet<String> =
        sqlx::query("SELECT `name` FROM `migrates`")
            .fetch_all(&pool)
            .await?
            .into_iter()
            .map(|r| r.get::<String, _>(0))
            .collect();

    let files = discover_up_files(&args.migrate_path)?;
    let mut applied_count = 0usize;
    loop {
        let next = files
            .iter()
            .find(|(n, _)| !applied_set.contains(n))
            .cloned();
        let (name, path) = match next {
            Some(t) => t,
            None => {
                if applied_count == 0 {
                    println!("No pending migrations.");
                } else {
                    println!("No more pending migrations.");
                }
                return Ok(());
            }
        };
        let sql = fs::read_to_string(&path)?;
        let sum = checksum_hex(&sql);

        // why no transaction: MySQL DDL auto-commits, so wrapping apply+INSERT gives no atomicity guarantee — mirror runUp's sequential execute+INSERT and accept the connection-failure window.
        for stmt in split_statements(&sql) {
            pool.execute(stmt.as_str()).await?;
        }
        sqlx::query("INSERT INTO `migrates` (`name`, `checksum`) VALUES (?, ?)")
            .bind(&name)
            .bind(&sum)
            .execute(&pool)
            .await?;
        println!("Applied: {}", name);
        applied_set.insert(name);
        applied_count += 1;
        if args.one {
            return Ok(());
        }
    }
}
// === END DIALECT_RUNNER_FNS_RUST ===

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Missing .env is the normal case (containers set real env vars); a malformed one aborts rather than half-configuring the run.
    match dotenvy::from_path(".env") {
        Ok(()) => {}
        Err(dotenvy::Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("failed to load .env: {err}").into()),
    }
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{}", msg);
            process::exit(2);
        }
    };
    if args.provider == "sqlite" {
        if let Some(path) = sqlite_filesystem_path(&args.connection) {
            if !std::path::Path::new(&path).exists() {
                eprintln!(
                    "sqlite file: {} does not exist — run 'migrate-setup --provider sqlite --connection {}' to create it",
                    path, path,
                );
                process::exit(2);
            }
        }
    }
    match args.provider.as_str() {
        // === BEGIN DIALECT_UP_DISPATCH_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
        "sqlite" => run_sqlite(&args).await?,
        "postgres" => run_postgres(&args).await?,
        "mysql" => run_mysql(&args).await?,
        // === END DIALECT_UP_DISPATCH_RUST ===
        other => return Err(format!("unsupported provider: {}", other).into()),
    }
    Ok(())
}
