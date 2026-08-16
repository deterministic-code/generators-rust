// Rolls back the most recently applied migration (one step). DESTRUCTIVE: prints a random 4-letter uppercase token and refuses to proceed unless the operator types it back on stdin. Pass --confirm <TOKEN> matching the printed token to bypass the prompt (CI / scripted use).

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

// === BEGIN DIALECT_USE_IMPORTS_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
use sqlx::{Executor, MySqlPool, PgPool, Row, SqlitePool};
// === END DIALECT_USE_IMPORTS_RUST ===

const HELP_TEXT: &str = r#"Usage: migrate-down --provider <sqlite|postgres|mysql> --connection <url> [--migrations-path <dir>] [--migrations-root <dir>] [--confirm <TOKEN>]

Rolls back the MOST RECENTLY applied migration (one step). DESTRUCTIVE: prints
a random 4-letter uppercase token on stderr and refuses to proceed unless the
operator types it back on stdin (or passes --confirm <TOKEN> matching it).

  --provider          Database dialect. Required.
  --connection        Connection string. Required.
  --migrations-path   Full path to the migrations directory.
  --migrations-root   Root prefix; the script then looks at <migrations-root>/<dialect>/migrations.
                      Defaults to 'sql'. Ignored when --migrations-path is also set.
  --confirm           Skip the interactive prompt. The TOKEN must match the value
                      printed on stderr at runtime — there is no fixed bypass value.

Examples:
  # sqlite — bare path or sqlite:// URL.
  migrate-down --provider sqlite --connection ./app.sqlite
  migrate-down --provider sqlite --connection sqlite:///absolute/path/app.sqlite

  # postgres — URL form (libpq keyword form is accepted by sqlx too).
  migrate-down --provider postgres --connection postgresql://user:pass@host:5432/dbname

  # mysql — URL form.
  migrate-down --provider mysql --connection mysql://user:pass@host:3306/dbname

  # custom migrations layout (default looks under sql/<dialect>/migrations):
  migrate-down --provider sqlite --connection ./app.sqlite --migrations-path ./db/changes/sqlite

Note: when --connection is omitted, ./.env is loaded (if present) and the
runner falls back to per-dialect env vars — sqlite: SQLITE_PATH, DB_PATH;
postgres: PG_CONNECTION_STRING, DATABASE_URL; mysql: MYSQL_URL, DATABASE_URL.
The typescript and csharp runners honor the same fallback.
"#;

struct Args {
    provider: String,
    migrate_path: String,
    connection: String,
    confirm: Option<String>,
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
    let mut confirm: Option<String> = None;
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
            "--confirm" => confirm = iter.next(),
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
    Ok(Args { provider, migrate_path, connection, confirm })
}

fn discover_down_files(
    migrate_path: &str,
) -> Result<std::collections::HashMap<String, PathBuf>, Box<dyn std::error::Error>> {
    let dir = PathBuf::from(migrate_path);
    if !dir.is_dir() {
        return Err(format!("migrate-path is not a directory: {}", migrate_path).into());
    }
    let mut out: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) if n.ends_with("_down.sql") => n.to_string(),
            _ => continue,
        };
        // Stem matches the `<stem>_up.sql` row recorded in `migrates.name`.
        let stem = name[..name.len() - "_down.sql".len()].to_string();
        let up_name = format!("{}_up.sql", stem);
        out.insert(up_name, path);
    }
    Ok(out)
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
        if !in_line_comment && !in_block_comment && in_dollar_tag.is_none() && !in_single_quote && !in_double_quote {
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

// Friction gate, not crypto. Seeded from system nanos — adequate to prevent muscle-memory `migrate-down && yes` flows; defenders against a determined adversary use --confirm.
fn random_token() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ"; // omit I, O — visually ambiguous
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut seed: u64 = ((nanos as u64) ^ (std::process::id() as u64)).max(1);
    let mut out = String::with_capacity(4);
    for _ in 0..4 {
        // xorshift64
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let idx = (seed as usize) % ALPHABET.len();
        out.push(ALPHABET[idx] as char);
    }
    out
}

fn confirm_or_abort(token: &str, confirm: Option<&str>) {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    let _ = writeln!(
        err,
        "\n⚠ DESTRUCTIVE: this rolls back the most recently applied migration.\n  Confirmation token: {}\n",
        token,
    );
    if let Some(supplied) = confirm {
        if supplied == token {
            return;
        }
        let _ = writeln!(
            err,
            "--confirm value ({}) does not match the token printed above ({}). Aborting.",
            supplied, token,
        );
        process::exit(2);
    }
    let _ = write!(err, "Type {} to confirm rollback: ", token);
    let _ = err.flush();
    let stdin = io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(_) => {}
        Err(_) => {
            let _ = writeln!(err, "failed to read confirmation from stdin — aborting.");
            process::exit(2);
        }
    }
    if line.trim() != token {
        let _ = writeln!(err, "Confirmation token did not match — aborting.");
        process::exit(2);
    }
}

// === BEGIN DIALECT_ROLLBACK_FNS_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
async fn rollback_sqlite(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePool::connect(&sqlite_url(&args.connection)).await?;
    let row = sqlx::query(r#"SELECT "name" FROM "migrates" ORDER BY "name" DESC LIMIT 1"#)
        .fetch_optional(&pool)
        .await?;
    let name = match row {
        Some(r) => r.get::<String, _>(0),
        None => {
            println!("No applied migrations to roll back.");
            return Ok(());
        }
    };
    let downs = discover_down_files(&args.migrate_path)?;
    let path = downs
        .get(&name)
        .ok_or_else(|| format!("Cannot roll back \"{}\": no <stem>_down.sql sibling found", name))?
        .clone();
    let sql = fs::read_to_string(&path)?;

    let mut tx = pool.begin().await?;
    for stmt in split_statements(&sql) {
        tx.execute(stmt.as_str()).await?;
    }
    sqlx::query(r#"DELETE FROM "migrates" WHERE "name" = ?"#)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    println!("Rolled back: {}", name);
    Ok(())
}

async fn rollback_postgres(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = PgPool::connect(&args.connection).await?;
    let row = sqlx::query(r#"SELECT "name" FROM "migrates" ORDER BY "name" DESC LIMIT 1"#)
        .fetch_optional(&pool)
        .await?;
    let name = match row {
        Some(r) => r.get::<String, _>(0),
        None => {
            println!("No applied migrations to roll back.");
            return Ok(());
        }
    };
    let downs = discover_down_files(&args.migrate_path)?;
    let path = downs
        .get(&name)
        .ok_or_else(|| format!("Cannot roll back \"{}\": no <stem>_down.sql sibling found", name))?
        .clone();
    let sql = fs::read_to_string(&path)?;

    let mut tx = pool.begin().await?;
    for stmt in split_statements(&sql) {
        tx.execute(stmt.as_str()).await?;
    }
    sqlx::query(r#"DELETE FROM "migrates" WHERE "name" = $1"#)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    println!("Rolled back: {}", name);
    Ok(())
}

async fn rollback_mysql(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let pool = MySqlPool::connect(&args.connection).await?;
    let row = sqlx::query("SELECT `name` FROM `migrates` ORDER BY `name` DESC LIMIT 1")
        .fetch_optional(&pool)
        .await?;
    let name = match row {
        Some(r) => r.get::<String, _>(0),
        None => {
            println!("No applied migrations to roll back.");
            return Ok(());
        }
    };
    let downs = discover_down_files(&args.migrate_path)?;
    let path = downs
        .get(&name)
        .ok_or_else(|| format!("Cannot roll back \"{}\": no <stem>_down.sql sibling found", name))?
        .clone();
    let sql = fs::read_to_string(&path)?;

    // why no transaction: MySQL DDL auto-commits, so wrapping apply+DELETE gives no atomicity guarantee — mirror runUp's pattern.
    for stmt in split_statements(&sql) {
        pool.execute(stmt.as_str()).await?;
    }
    sqlx::query("DELETE FROM `migrates` WHERE `name` = ?")
        .bind(&name)
        .execute(&pool)
        .await?;
    println!("Rolled back: {}", name);
    Ok(())
}
// === END DIALECT_ROLLBACK_FNS_RUST ===

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
    let token = random_token();
    confirm_or_abort(&token, args.confirm.as_deref());
    match args.provider.as_str() {
        // === BEGIN DIALECT_DOWN_DISPATCH_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
        "sqlite" => rollback_sqlite(&args).await?,
        "postgres" => rollback_postgres(&args).await?,
        "mysql" => rollback_mysql(&args).await?,
        // === END DIALECT_DOWN_DISPATCH_RUST ===
        other => return Err(format!("unsupported provider: {}", other).into()),
    }
    Ok(())
}
