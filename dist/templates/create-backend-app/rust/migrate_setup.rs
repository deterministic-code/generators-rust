// Idempotent CREATE TABLE IF NOT EXISTS migrates+migrate_logs; mirrors scripts/lib/datasource-migrate.mjs:setupSql so rust + TS containers share one migrates contract.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

// === BEGIN DIALECT_USE_IMPORTS_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
use sqlx::{MySqlPool, PgPool, SqlitePool};
// === END DIALECT_USE_IMPORTS_RUST ===

// === BEGIN DIALECT_DDL_CONSTS_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
const SQLITE_MIGRATES_DDL: &str = r#"CREATE TABLE IF NOT EXISTS "migrates" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "name" VARCHAR(255) NOT NULL UNIQUE,
  "checksum" VARCHAR(64),
  "created" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);"#;

const SQLITE_MIGRATE_LOGS_DDL: &str = r#"CREATE TABLE IF NOT EXISTS "migrate_logs" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "migrate_name" VARCHAR(255) NOT NULL,
  "direction" VARCHAR(8) NOT NULL,
  "status" VARCHAR(16) NOT NULL,
  "finished_at" DATETIME,
  "duration_ms" INTEGER,
  "error_message" TEXT,
  "created" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updated" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);"#;

const POSTGRES_MIGRATES_DDL: &str = r#"CREATE TABLE IF NOT EXISTS "migrates" (
  "id" BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  "name" VARCHAR(255) NOT NULL UNIQUE,
  "checksum" VARCHAR(64),
  "created" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  "updated" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);"#;

const POSTGRES_MIGRATE_LOGS_DDL: &str = r#"CREATE TABLE IF NOT EXISTS "migrate_logs" (
  "id" BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  "migrate_name" VARCHAR(255) NOT NULL,
  "direction" VARCHAR(8) NOT NULL,
  "status" VARCHAR(16) NOT NULL,
  "finished_at" TIMESTAMPTZ,
  "duration_ms" INTEGER,
  "error_message" TEXT,
  "created" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  "updated" TIMESTAMPTZ NOT NULL DEFAULT NOW()
);"#;

const MYSQL_MIGRATES_DDL: &str = r#"CREATE TABLE IF NOT EXISTS `migrates` (
  `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
  `name` VARCHAR(255) NOT NULL UNIQUE,
  `checksum` VARCHAR(64),
  `created` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `updated` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);"#;

const MYSQL_MIGRATE_LOGS_DDL: &str = r#"CREATE TABLE IF NOT EXISTS `migrate_logs` (
  `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
  `migrate_name` VARCHAR(255) NOT NULL,
  `direction` VARCHAR(8) NOT NULL,
  `status` VARCHAR(16) NOT NULL,
  `finished_at` TIMESTAMP NULL,
  `duration_ms` INT,
  `error_message` TEXT,
  `created` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `updated` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);"#;
// === END DIALECT_DDL_CONSTS_RUST ===

const HELP_TEXT: &str = r#"Usage: migrate-setup --provider <sqlite|postgres|mysql> --connection <url> [--migrations-path <dir>] [--and-up]

Creates the bookkeeping tables (migrates, migrate_logs) for the chosen dialect
and the migrations directory (mkdir -p). Idempotent — re-running against an
already-setup database is a no-op. For sqlite, also mkdir -p of the parent
directory of --connection so a fresh-tree path like ./.test/foo.sqlite works.

  --provider          Database dialect. Required.
  --connection        Connection string. Required.
  --migrations-path   Migrations directory (default: ./sql/<dialect>/migrations).
  --and-up            After setup, immediately invoke migrate-up so the chain is applied in one step.

Examples:
  # sqlite — bare path or sqlite:// URL; both create a file on disk.
  migrate-setup --provider sqlite --connection ./app.sqlite
  migrate-setup --provider sqlite --connection sqlite:///absolute/path/app.sqlite

  # postgres — URL form (libpq keyword form is accepted by sqlx too).
  migrate-setup --provider postgres --connection postgresql://user:pass@host:5432/dbname

  # mysql — URL form.
  migrate-setup --provider mysql --connection mysql://user:pass@host:3306/dbname

Note: when --connection is omitted, ./.env is loaded (if present) and the
runner falls back to per-dialect env vars — sqlite: SQLITE_PATH, DB_PATH;
postgres: PG_CONNECTION_STRING, DATABASE_URL; mysql: MYSQL_URL, DATABASE_URL.
The typescript and csharp runners honor the same fallback.
"#;

struct Args {
    provider: String,
    connection: String,
    migrations_path: String,
    and_up: bool,
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
    let mut connection: Option<String> = None;
    let mut migrations_path: Option<String> = None;
    let mut and_up = false;
    let mut iter = env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--provider" => provider = iter.next(),
            "--connection" => connection = iter.next(),
            // --migrate-path is a silent alias retained for backward compat.
            "--migrations-path" => migrations_path = iter.next(),
            "--migrate-path" => migrations_path = iter.next(),
            "--and-up" => and_up = true,
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
    let connection = match connection.or_else(|| connection_from_env(&provider)) {
        Some(c) => c,
        None => {
            return Err(format!(
                "missing --connection — pass --connection <url> (e.g. ./app.sqlite for sqlite) or set one of: {}. Run with --help for examples.",
                connection_env_vars(&provider).join(", ")
            ))
        }
    };
    let migrations_path =
        migrations_path.unwrap_or_else(|| format!("./sql/{}/migrations", provider));
    Ok(Args {
        provider,
        connection,
        migrations_path,
        and_up,
    })
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

fn sqlite_filesystem_path(connection: &str) -> Option<PathBuf> {
    let s = connection.trim();
    let stripped = if let Some(rest) = s.strip_prefix("sqlite://") {
        rest
    } else if let Some(rest) = s.strip_prefix("sqlite:") {
        rest
    } else if let Some(rest) = s.strip_prefix("file:") {
        rest
    } else {
        s
    };
    if stripped.is_empty() || stripped == ":memory:" {
        return None;
    }
    let without_query = stripped.split('?').next().unwrap_or(stripped);
    Some(PathBuf::from(without_query))
}

fn sibling_migrate_up_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let me = env::current_exe()?;
    let dir = me.parent().ok_or("current_exe has no parent")?;
    let mut name = String::from("migrate-up");
    if let Some(ext) = me.extension().and_then(|e| e.to_str()) {
        name.push('.');
        name.push_str(ext);
    }
    Ok(dir.join(name))
}

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
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
        }
    }
    match args.provider.as_str() {
        // === BEGIN DIALECT_SETUP_DISPATCH_RUST — see PATCH_PLAN in create-migrate-scripts.mjs ===
        "sqlite" => {
            let pool = SqlitePool::connect(&sqlite_url(&args.connection)).await?;
            sqlx::query(SQLITE_MIGRATES_DDL).execute(&pool).await?;
            sqlx::query(SQLITE_MIGRATE_LOGS_DDL).execute(&pool).await?;
        }
        "postgres" => {
            let pool = PgPool::connect(&args.connection).await?;
            sqlx::query(POSTGRES_MIGRATES_DDL).execute(&pool).await?;
            sqlx::query(POSTGRES_MIGRATE_LOGS_DDL)
                .execute(&pool)
                .await?;
        }
        "mysql" => {
            let pool = MySqlPool::connect(&args.connection).await?;
            sqlx::query(MYSQL_MIGRATES_DDL).execute(&pool).await?;
            sqlx::query(MYSQL_MIGRATE_LOGS_DDL).execute(&pool).await?;
        }
        // === END DIALECT_SETUP_DISPATCH_RUST ===
        other => return Err(format!("unsupported provider: {}", other).into()),
    }
    fs::create_dir_all(PathBuf::from(&args.migrations_path))?;
    println!(
        "Setup complete: migrates and migrate_logs ready ({}).",
        args.provider
    );
    println!("Migrations directory: {}", args.migrations_path);
    if !args.and_up {
        println!();
        println!("Next steps:");
        println!("  # create a new migration");
        println!(
            "  cargo run --release --bin migrate-create -- --provider {} --name add_users",
            args.provider,
        );
        println!();
        println!("  # apply pending migrations");
        println!(
            "  cargo run --release --bin migrate-up -- --provider {} --connection {}",
            args.provider, args.connection,
        );
    }
    if args.and_up {
        let up = sibling_migrate_up_binary()?;
        let status = std::process::Command::new(&up)
            .arg("--provider")
            .arg(&args.provider)
            .arg("--connection")
            .arg(&args.connection)
            .arg("--migrations-path")
            .arg(&args.migrations_path)
            .status()?;
        if !status.success() {
            process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}
