import { readFile } from "node:fs/promises";
import { fill } from "../common/fill.ts";

const chunk = async (name: string): Promise<string> =>
  (
    await readFile(
      new URL(`../templates/create-backend-app/rust/chunks/${name}`, import.meta.url),
      "utf8",
    )
  ).trimEnd();

const filterChunks = (
  chunks: Record<string, string>,
  dialects: string[],
  joiner = "\n",
): string =>
  dialects
    .filter((d) => chunks[d])
    .map((d) => chunks[d])
    .join(joiner);

const SQLX_FEATURE_BY_DIALECT: Record<string, string> = {
  sqlite: "sqlite",
  postgres: "postgres",
  mysql: "mysql",
};

const rustSqlxDepLine = (dialects: string[]): string => {
  const features = ["runtime-tokio"];
  for (const d of dialects) {
    const f = SQLX_FEATURE_BY_DIALECT[d];
    if (f && !features.includes(f)) features.push(f);
  }
  return `sqlx = { version = "0.8", default-features = false, features = [${features.map((f) => `"${f}"`).join(", ")}] }`;
};

const [
  sqliteUrlHelper,
  ddlConstsSqlite,
  ddlConstsPostgres,
  ddlConstsMysql,
  setupDispatchTmpl,
  runnerFnSqlite,
  runnerFnPostgres,
  runnerFnMysql,
  upDispatchTmpl,
  rollbackFnSqlite,
  rollbackFnPostgres,
  rollbackFnMysql,
  downDispatchTmpl,
] = await Promise.all([
  chunk("sqlite_url_helper.rs"),
  chunk("ddl_consts_sqlite.rs"),
  chunk("ddl_consts_postgres.rs"),
  chunk("ddl_consts_mysql.rs"),
  chunk("setup_dispatch.rs"),
  chunk("runner_fn_sqlite.rs"),
  chunk("runner_fn_postgres.rs"),
  chunk("runner_fn_mysql.rs"),
  chunk("up_dispatch.rs"),
  chunk("rollback_fn_sqlite.rs"),
  chunk("rollback_fn_postgres.rs"),
  chunk("rollback_fn_mysql.rs"),
  chunk("down_dispatch.rs"),
]);

const RUST_DDL_CONST_CHUNKS = {
  sqlite: ddlConstsSqlite,
  postgres: ddlConstsPostgres,
  mysql: ddlConstsMysql,
};

const RUST_SETUP_DISPATCH_CHUNKS = {
  sqlite: fill(setupDispatchTmpl, {
    Dialect: "sqlite",
    PoolCtor: "SqlitePool::connect(&sqlite_url(&args.connection)).await?",
    Prefix: "SQLITE",
  }),
  postgres: fill(setupDispatchTmpl, {
    Dialect: "postgres",
    PoolCtor: "PgPool::connect(&args.connection).await?",
    Prefix: "POSTGRES",
  }),
  mysql: fill(setupDispatchTmpl, {
    Dialect: "mysql",
    PoolCtor: "MySqlPool::connect(&args.connection).await?",
    Prefix: "MYSQL",
  }),
};

const RUST_RUNNER_FN_CHUNKS = {
  sqlite: runnerFnSqlite,
  postgres: runnerFnPostgres,
  mysql: runnerFnMysql,
};

const RUST_UP_DISPATCH_CHUNKS = {
  sqlite: fill(upDispatchTmpl, { Dialect: "sqlite" }),
  postgres: fill(upDispatchTmpl, { Dialect: "postgres" }),
  mysql: fill(upDispatchTmpl, { Dialect: "mysql" }),
};

const RUST_ROLLBACK_FN_CHUNKS = {
  sqlite: rollbackFnSqlite,
  postgres: rollbackFnPostgres,
  mysql: rollbackFnMysql,
};

const RUST_DOWN_DISPATCH_CHUNKS = {
  sqlite: fill(downDispatchTmpl, { Dialect: "sqlite" }),
  postgres: fill(downDispatchTmpl, { Dialect: "postgres" }),
  mysql: fill(downDispatchTmpl, { Dialect: "mysql" }),
};

export const buildRustCargoTomlDepsBlock = (dialects: string[]): string =>
  `${rustSqlxDepLine(dialects)}
dotenvy = "0.15"`;

const rustDialectPoolTypes = (dialects: string[]): string[] => {
  const has = (d: string) => dialects.includes(d);
  const parts: string[] = [];
  if (has("sqlite")) parts.push("SqlitePool");
  if (has("postgres")) parts.push("PgPool");
  if (has("mysql")) parts.push("MySqlPool");
  return parts;
};

export const rustDialectUseImportsForSetup = (dialects: string[]): string =>
  `use sqlx::{${rustDialectPoolTypes(dialects).join(", ")}};`;

export const rustDialectUseImportsForRunners = (dialects: string[]): string => {
  const parts = ["Executor", "Row", ...rustDialectPoolTypes(dialects)];
  return `use sqlx::{${parts.join(", ")}};`;
};

export const rustDialectSqliteUrlHelperBlock = (dialects: string[]): string =>
  dialects.includes("sqlite") ? sqliteUrlHelper : "";

export const rustDialectDdlConstsBlock = (dialects: string[]): string =>
  filterChunks(RUST_DDL_CONST_CHUNKS, dialects, "\n\n");

export const rustDialectSetupDispatchBlock = (dialects: string[]): string =>
  filterChunks(RUST_SETUP_DISPATCH_CHUNKS, dialects, "\n");

export const rustDialectRunnerFnsBlock = (dialects: string[]): string =>
  filterChunks(RUST_RUNNER_FN_CHUNKS, dialects, "\n\n");

export const rustDialectUpDispatchBlock = (dialects: string[]): string =>
  filterChunks(RUST_UP_DISPATCH_CHUNKS, dialects, "\n");

export const rustDialectRollbackFnsBlock = (dialects: string[]): string =>
  filterChunks(RUST_ROLLBACK_FN_CHUNKS, dialects, "\n\n");

export const rustDialectDownDispatchBlock = (dialects: string[]): string =>
  filterChunks(RUST_DOWN_DISPATCH_CHUNKS, dialects, "\n");
