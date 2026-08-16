// Rust `create_backend_app` emitter: src/main.rs boots the runtime's dynamic server (deterministic::run(RunConfig::from_env())) plus the surrounding scaffold (Cargo.toml / Dockerfile / docker-compose / entrypoint / .env / .env.example / .gitignore). The previous health-only stub main + APP_* marker patching was deleted: the patch silently missed in synthesized crates and stub servers shipped to production.

import { readdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { SECTION_MARKERS } from "@deterministic-code/generator-sdk/section-markers";
import { RUST_APP_DEPS, RUST_APP_DEV_DEPS } from "./rust-deps.ts";
import { DEV_PORTS } from "@deterministic-code/generator-sdk/create-backend-app-model";
import {
  loadChunk,
  applyTokens,
  renderDialectMap,
} from "@deterministic-code/generator-sdk/codegen/lib/chunk-loader";
import { filterChunks } from "@deterministic-code/generator-sdk/dialect-filter";
import { rustSqlxDepLine } from "@deterministic-code/generator-sdk/lib/migrate-scripts-plan";
import {
  COMPOSE_FILENAME,
  renderRustStandaloneComposeService,
  renderRustComposeService,
} from "@deterministic-code/generator-sdk/codegen/lib/compose-services";
import { isMultiLanguage } from "@deterministic-code/generator-sdk/codegen/lib/declared-languages";
import { backendLaneDir } from "@deterministic-code/generator-sdk/codegen/lib/backend-lane";
import { appWiringComposePath } from "../codegen/lib/rust-crate-paths.ts";
import {
  DOCKERIGNORE_TRIGGER,
  dockerignoreSection,
} from "@deterministic-code/patch-merger";
import { REPO_ROOT, firstExistingDir } from "@deterministic-code/generator-sdk/codegen/lib/artifact-paths";
import type { EmitEntry } from "@deterministic-code/generator-sdk/codegen/lib/emit-result";
import {
  CONTENT,
  PATCH,
  skeletonEntriesFromFiles,
} from "@deterministic-code/generator-sdk/codegen/lib/emit-result";
import {
  readSettingsWithDefault,
  resolveLibraryReferenceMode,
} from "@deterministic-code/generator-sdk/read-settings";

interface EmitArgs {
  input?: string;
  deterministicDir?: string;
  combined?: boolean;
  language?: string;
}

interface RustCrateFile {
  path: string;
  content: string;
  mode?: number;
}

interface RegistryOverrides {
  indexUrl?: string;
  registryName?: string;
}

const here = dirname(fileURLToPath(import.meta.url));
const RUST_ENTRYPOINT_TEMPLATE_PATH = resolve(
  here,
  "..",
  "templates",
  "create-backend-app",
  "rust",
  "entrypoint.sh",
);

const MAIN_RS_CHUNK = await loadChunk("rust", "main");
const CARGO_TOML_CHUNK = await loadChunk("rust", "cargo.toml");
const DOCKERFILE_CHUNK = await loadChunk("rust", "dockerfile.txt");

/** Lib + bin target name of every emitted consumer crate; main.rs (bin) references generated modules through it. The runtime dep keeps its natural name `deterministic`, so emitted code resolves it from the extern prelude everywhere — no extern-crate alias. */
export const RUST_SYNTHESIZED_CRATE_NAME = "generated";

export function emitMainRs({
  packageName = RUST_SYNTHESIZED_CRATE_NAME,
}: { packageName?: string } = {}) {
  const content = applyTokens(MAIN_RS_CHUNK, { packageName }) + "\n";
  return { path: "src/main.rs", content };
}

/** Library entry point: the MODULES block starts empty (each module step patches its own `pub mod X;` line in) and custom_services() lives here — not main.rs — so cargo tests booting via build_app register the same custom services the bin does. `route_composer()` resolves the app-wiring aggregator whose module path tracks the layout (by-feature vs flat). */
export function emitLibRs({ byFeature = false }: { byFeature?: boolean } = {}) {
  const content = `${SECTION_MARKERS.MODULES.start}
${SECTION_MARKERS.MODULES.end}

pub fn custom_services() -> deterministic::CustomServices {
    let services = deterministic::CustomServices::new();
    ${SECTION_MARKERS.CUSTOM_SERVICES.start}
    ${SECTION_MARKERS.CUSTOM_SERVICES.end}
    services
}

pub fn route_composer() -> deterministic::RouteComposer {
    deterministic::RouteComposer::new(${appWiringComposePath(byFeature)})
}
`;
  return { path: "src/lib.rs", content };
}

export const DEFAULT_REGISTRY_NAME = "deterministic-code";
export const DEFAULT_REGISTRY_INDEX_URL =
  "sparse+https://deterministic-code.com/registry/";

export function renderCargoToml({
  packageName = "generated",
  libraryReferenceMode = "bundled",
  crateVersion = null,
  registryName = DEFAULT_REGISTRY_NAME,
}: {
  packageName?: string;
  libraryReferenceMode?: string;
  crateVersion?: string | null;
  registryName?: string;
} = {}) {
  // `time = "=0.3.47"` pins a transitive sqlx dep: time 0.3.50 broke with `error[E0432]: unresolved import time_macros::timestamp` (time-macros version skew). Matches main's Cargo.lock; revisit when upstream ships a fix.
  const detDep = renderDeterministicDep({
    libraryReferenceMode,
    crateVersion,
    registryName,
  });
  return (
    applyTokens(CARGO_TOML_CHUNK, {
      packageName,
      detDep,
      appDeps: RUST_APP_DEPS,
      appDevDeps: RUST_APP_DEV_DEPS,
      MIGRATE_BIN_START: SECTION_MARKERS.MIGRATE_BIN.start,
      MIGRATE_BIN_END: SECTION_MARKERS.MIGRATE_BIN.end,
      MIGRATE_DEPS_START: SECTION_MARKERS.MIGRATE_DEPS.start,
      MIGRATE_DEPS_END: SECTION_MARKERS.MIGRATE_DEPS.end,
      PERF_BIN_START: SECTION_MARKERS.PERF_BIN.start,
      PERF_BIN_END: SECTION_MARKERS.PERF_BIN.end,
    }) + "\n"
  );
}

function renderDeterministicDep({
  libraryReferenceMode,
  crateVersion,
  registryName,
}: {
  libraryReferenceMode?: string;
  crateVersion?: string | null;
  registryName?: string;
}) {
  if (libraryReferenceMode === "registry") {
    if (!crateVersion) {
      throw new Error(
        "renderCargoToml: libraryReferenceMode=registry requires a crateVersion (read from the library's rust/Cargo.toml at codegen time)",
      );
    }
    return `deterministic = { version = "= ${crateVersion}", registry = "${registryName}" }`;
  }
  return `deterministic = { path = "_deterministic/rust" }`;
}

/** why .cargo/config.toml at crate root: cargo discovers alternative registries via this file (or a global ~/.cargo/config.toml). Emitting it sibling to Cargo.toml makes the consumer crate self-contained — `cargo build` in a fresh clone of the emitted project finds the registry without any out-of-band setup. */
export function renderRustCargoConfig({
  registryName = DEFAULT_REGISTRY_NAME,
  indexUrl = DEFAULT_REGISTRY_INDEX_URL,
}: { registryName?: string; indexUrl?: string } = {}) {
  return `[registries.${registryName}]
index = "${indexUrl}"
`;
}

/** why builder-based runtime + self-diagnosing HEALTHCHECK: the runtime stage extends the rust builder (keeps cargo + sources) so verify's in-container `cargo test` runs — a debian-slim runner strips cargo; HEALTHCHECK shape mirrors the TS Dockerfile so a failed probe shows a usable status code instead of "(no output)" in docker inspect. In multi-lang the root compose sets `context: .` + `dockerfile: ./rust/Dockerfile`, so lane-relative COPY lines (Cargo.toml, src, _deterministic/.cargo, scripts) carry the `rust/` prefix while root-shared deterministic/ and the migrate-patched `COPY sql` stay reachable from the root context. */
export function renderRustDockerfile({
  binaryName = "generated",
  libraryReferenceMode = "bundled",
  multiLanguage = false,
  laneDir,
}: {
  binaryName?: string;
  libraryReferenceMode?: string;
  multiLanguage?: boolean;
  laneDir?: string;
} = {}) {
  // laneDir is the crate's location relative to the root build context: `backend/rust/` combined, `rust/` multi-lang-only, `` standalone. It defaults from multiLanguage so existing callers stay byte-identical; only combined generation passes the `backend/`-prefixed form.
  const rustPrefix = laneDir ?? (multiLanguage ? "rust/" : "");
  // why two COPY shapes: bundled mode ships the runtime in _deterministic/; registry mode pulls it over the network at build time and needs only `.cargo/config.toml` available so cargo sees the alternative-registry definition.
  const sourceCopy =
    libraryReferenceMode === "registry"
      ? `COPY ${rustPrefix}.cargo ./.cargo`
      : `COPY ${rustPrefix}_deterministic ./_deterministic`;
  return (
    applyTokens(DOCKERFILE_CHUNK, {
      binaryName,
      sourceCopy,
      rustPrefix,
      MIGRATE_COPY_START: SECTION_MARKERS.MIGRATE_COPY.start,
      MIGRATE_COPY_END: SECTION_MARKERS.MIGRATE_COPY.end,
      MIGRATE_RUNTIME_COPY_START: SECTION_MARKERS.MIGRATE_RUNTIME_COPY.start,
      MIGRATE_RUNTIME_COPY_END: SECTION_MARKERS.MIGRATE_RUNTIME_COPY.end,
    }) + "\n"
  );
}

/** why read template file: the MIGRATE_HOOK markers are the contract the migrate_scripts patcher requires — inlining a barebones string drifts from the template and patchEntrypoint then throws "create-backend-app template drift". */
async function renderRustEntrypoint(): Promise<string> {
  return await readFile(RUST_ENTRYPOINT_TEMPLATE_PATH, "utf8");
}

function renderRustEnvSection(): string {
  return `PORT=${DEV_PORTS.rust}
DETERMINISTIC_DIR=./deterministic
`;
}

function renderRustGitignoreSection(): string {
  return `target/
_deterministic/rust/target/
`;
}

export async function readLibraryCrateVersion(): Promise<string> {
  const cargoPath = resolve(REPO_ROOT, "rust", "Cargo.toml");
  const text = await readFile(cargoPath, "utf8");
  const m = text.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!m) {
    throw new Error(
      `readLibraryCrateVersion: could not parse a version line out of ${cargoPath}`,
    );
  }
  return m[1];
}

/** why env-driven (not CLI flags): the e2e spec needs to point the emitted .cargo/config.toml at an in-process registry whose URL is only known at test-time (free-port allocation), and inject `host.docker.internal:host-gateway` so cargo inside the container reaches the host backend. Production deploys ignore both — defaults stay https://deterministic-code.com/registry and no extra_hosts. */
function registryConfigOverridesFromEnv(): RegistryOverrides {
  const indexUrl = process.env.DETERMINISTIC_CRATE_REGISTRY_INDEX_URL;
  const registryName = process.env.DETERMINISTIC_CRATE_REGISTRY_NAME;
  const opts: RegistryOverrides = {};
  if (indexUrl) opts.indexUrl = indexUrl;
  if (registryName) opts.registryName = registryName;
  return opts;
}

function dockerExtraHostsFromEnv(): string[] {
  const raw = process.env.DETERMINISTIC_CRATE_REGISTRY_EXTRA_HOSTS;
  if (!raw) return [];
  return raw
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

interface RustCrateFilesOpts {
  libraryReferenceMode: string;
  multiLanguage: boolean;
  laneDir: string;
  byFeature: boolean;
}

async function buildRustCrateFiles({
  libraryReferenceMode,
  multiLanguage,
  laneDir,
  byFeature,
}: RustCrateFilesOpts): Promise<RustCrateFile[]> {
  const crateVersion =
    libraryReferenceMode === "registry"
      ? await readLibraryCrateVersion()
      : null;
  const registryOverrides = registryConfigOverridesFromEnv();
  const files: RustCrateFile[] = [
    emitMainRs(),
    emitLibRs({ byFeature }),
    {
      path: "Cargo.toml",
      content: renderCargoToml({
        packageName: RUST_SYNTHESIZED_CRATE_NAME,
        libraryReferenceMode,
        crateVersion,
        ...(registryOverrides.registryName
          ? { registryName: registryOverrides.registryName }
          : {}),
      }),
    },
    {
      path: "scripts/entrypoint.sh",
      content: await renderRustEntrypoint(),
      mode: 0o755,
    },
  ];
  if (libraryReferenceMode === "registry") {
    files.push({
      path: ".cargo/config.toml",
      content: renderRustCargoConfig(registryOverrides),
    });
  }
  files.push({
    path: "Dockerfile",
    content: renderRustDockerfile({
      binaryName: RUST_SYNTHESIZED_CRATE_NAME,
      libraryReferenceMode,
      multiLanguage,
      laneDir,
    }),
  });
  return files;
}

async function bundledRuntimeEntries(): Promise<EmitEntry[]> {
  const rustSrc = await resolveLibraryRustDir(REPO_ROOT);
  return readTreeEntries(rustSrc, "_deterministic/rust", rustBundleExclude);
}

async function readTreeEntries(
  srcDir: string,
  destPrefix: string,
  exclude: (relPath: string) => boolean,
): Promise<EmitEntry[]> {
  const out: EmitEntry[] = [];
  const walk = async (dir: string, rel: string) => {
    for (const ent of await readdir(dir, { withFileTypes: true })) {
      const relPath = rel ? `${rel}/${ent.name}` : ent.name;
      if (exclude(relPath)) continue;
      if (ent.isDirectory()) {
        await walk(join(dir, ent.name), relPath);
        continue;
      }
      out.push({
        kind: CONTENT,
        filename: `${destPrefix}/${relPath}`,
        contents: await readFile(join(dir, ent.name), "utf8"),
      });
    }
  };
  await walk(srcDir, "");
  return out;
}

async function resolveLibraryRustDir(rootDir: string): Promise<string> {
  return firstExistingDir(
    [
      join(
        rootDir,
        "node_modules",
        "@deterministic-code",
        "deterministic",
        "rust",
      ),
      join(rootDir, "rust"),
    ],
    `library_reference_mode=bundled (rust) requires @deterministic-code/deterministic to be installed or the rust/ crate to be present in the repo.`,
  );
}

function rustBundleExclude(relativeFromSrc: string): boolean {
  if (relativeFromSrc === "target") return true;
  if (relativeFromSrc.startsWith("target/")) return true;
  if (relativeFromSrc === "tests") return true;
  if (relativeFromSrc.startsWith("tests/")) return true;
  if (relativeFromSrc === "Cargo.lock") return true;
  if (relativeFromSrc.endsWith(".sqlite")) return true;
  if (relativeFromSrc.endsWith(".db")) return true;
  return false;
}

/** Cargo.toml / Dockerfile / entrypoint.sh / lib.rs are composed skeletons — backend_app emits the base piece (with MIGRATE_BIN/… or the MODULES/CUSTOM_SERVICES markers) and the per-module/migrate/perf steps fill the marked regions; everything else is single-contributor content. */
const SKELETON_TARGETS = new Set([
  "Cargo.toml",
  "Dockerfile",
  "scripts/entrypoint.sh",
  "src/lib.rs",
]);

/** The single-contributor scaffold patches (compose service / .env / .gitignore / .dockerignore trigger) every rust backend app emits alongside its composed-skeleton crate files. */
function scaffoldPatchEntries(laneDir: string): EmitEntry[] {
  return [
    {
      kind: PATCH,
      filename: COMPOSE_FILENAME,
      content: laneDir
        ? renderRustComposeService(
            // compose-services.mjs infers dockerfilePath as null from its default; the runtime accepts the string path.
            {
              dockerfilePath: `./${laneDir}Dockerfile`,
            } as unknown as Parameters<typeof renderRustComposeService>[0],
          )
        : renderRustStandaloneComposeService(
            // compose-services.mjs infers extraHosts as never[] from its default; the runtime accepts the string list.
            {
              extraHosts: dockerExtraHostsFromEnv(),
            } as unknown as Parameters<
              typeof renderRustStandaloneComposeService
            >[0],
          ),
    },
    { kind: PATCH, filename: ".env", content: renderRustEnvSection() },
    { kind: PATCH, filename: ".env.example", content: renderRustEnvSection() },
    {
      kind: PATCH,
      filename: ".gitignore",
      content: renderRustGitignoreSection(),
    },
    {
      kind: PATCH,
      filename: ".dockerignore",
      section: dockerignoreSection("rust"),
      content: DOCKERIGNORE_TRIGGER,
    },
  ];
}

export async function emitBackendApp(args: EmitArgs): Promise<EmitEntry[]> {
  const inputPath = args.deterministicDir ?? args.input;
  if (typeof inputPath !== "string") {
    throw new Error("create-backend-app (rust): input is required");
  }
  const settings = await readSettingsWithDefault(resolve(inputPath));
  const libraryReferenceMode = resolveLibraryReferenceMode(
    settings.languages,
    "rust",
  );
  const multiLanguage = isMultiLanguage(settings);
  const byFeature = settings.other.organizeByFeature === true;
  // The crate's location relative to the root build context: combined generation nests it at backend/rust/, multi-language-only at rust/. Both the Dockerfile's own COPY prefixes and the root compose service's dockerfile: field must agree with it.
  const laneDir = backendLaneDir({
    combined: args.combined === true,
    multiLanguage,
    language: "rust",
  });
  const entries: EmitEntry[] = skeletonEntriesFromFiles(
    await buildRustCrateFiles({
      libraryReferenceMode,
      multiLanguage,
      laneDir,
      byFeature,
    }),
    (path) => SKELETON_TARGETS.has(path),
  );
  entries.push(...scaffoldPatchEntries(laneDir));
  if (libraryReferenceMode === "bundled") {
    entries.push(...(await bundledRuntimeEntries()));
  }
  return entries;
}

export function buildRustCargoTomlDepsBlock(dialects: string[] = []): string {
  // dotenvy: the migrate bins load ./.env; in combined scope they compile inside the app crate, so the dep rides MIGRATE_DEPS (standalone scope gets it from migrate-Cargo.toml).
  return `${rustSqlxDepLine(dialects)}
dotenvy = "0.15"`;
}

function rustDialectPoolTypes(dialects: string[] = []): string[] {
  const has = (d: string) => dialects.includes(d);
  const parts: string[] = [];
  if (has("sqlite")) parts.push("SqlitePool");
  if (has("postgres")) parts.push("PgPool");
  if (has("mysql")) parts.push("MySqlPool");
  return parts;
}

export function rustDialectUseImportsForSetup(dialects: string[] = []): string {
  return `use sqlx::{${rustDialectPoolTypes(dialects).join(", ")}};`;
}

export function rustDialectUseImportsForRunners(
  dialects: string[] = [],
): string {
  const parts = ["Executor", "Row", ...rustDialectPoolTypes(dialects)];
  return `use sqlx::{${parts.join(", ")}};`;
}

const RUST_SQLITE_URL_HELPER = await loadChunk("rust", "sqlite_url_helper");

export function rustDialectSqliteUrlHelperBlock(
  dialects: string[] = [],
): string {
  return dialects.includes("sqlite") ? RUST_SQLITE_URL_HELPER : "";
}

const RUST_DDL_CONST_CHUNKS = {
  sqlite: await loadChunk("rust", "ddl_consts_sqlite"),
  postgres: await loadChunk("rust", "ddl_consts_postgres"),
  mysql: await loadChunk("rust", "ddl_consts_mysql"),
};

export function rustDialectDdlConstsBlock(dialects: string[] = []): string {
  return filterChunks(RUST_DDL_CONST_CHUNKS, dialects, "\n\n");
}

const RUST_SETUP_DISPATCH_TOKENS = {
  sqlite: {
    Dialect: "sqlite",
    PoolCtor: "SqlitePool::connect(&sqlite_url(&args.connection)).await?",
    Prefix: "SQLITE",
  },
  postgres: {
    Dialect: "postgres",
    PoolCtor: "PgPool::connect(&args.connection).await?",
    Prefix: "POSTGRES",
  },
  mysql: {
    Dialect: "mysql",
    PoolCtor: "MySqlPool::connect(&args.connection).await?",
    Prefix: "MYSQL",
  },
};

const RUST_SETUP_DISPATCH_CHUNKS = (await renderDialectMap(
  "rust",
  "setup_dispatch",
  RUST_SETUP_DISPATCH_TOKENS,
)) as Record<string, string>;

export function rustDialectSetupDispatchBlock(dialects: string[] = []): string {
  return filterChunks(RUST_SETUP_DISPATCH_CHUNKS, dialects, "\n");
}

const RUST_RUNNER_FN_CHUNKS = {
  sqlite: await loadChunk("rust", "runner_fn_sqlite"),
  postgres: await loadChunk("rust", "runner_fn_postgres"),
  mysql: await loadChunk("rust", "runner_fn_mysql"),
};

export function rustDialectRunnerFnsBlock(dialects: string[] = []): string {
  return filterChunks(RUST_RUNNER_FN_CHUNKS, dialects, "\n\n");
}

const RUST_DIALECT_ONLY_TOKENS = {
  sqlite: { Dialect: "sqlite" },
  postgres: { Dialect: "postgres" },
  mysql: { Dialect: "mysql" },
};

const RUST_UP_DISPATCH_CHUNKS = (await renderDialectMap(
  "rust",
  "up_dispatch",
  RUST_DIALECT_ONLY_TOKENS,
)) as Record<string, string>;

export function rustDialectUpDispatchBlock(dialects: string[] = []): string {
  return filterChunks(RUST_UP_DISPATCH_CHUNKS, dialects, "\n");
}

const RUST_ROLLBACK_FN_CHUNKS = {
  sqlite: await loadChunk("rust", "rollback_fn_sqlite"),
  postgres: await loadChunk("rust", "rollback_fn_postgres"),
  mysql: await loadChunk("rust", "rollback_fn_mysql"),
};

export function rustDialectRollbackFnsBlock(dialects: string[] = []): string {
  return filterChunks(RUST_ROLLBACK_FN_CHUNKS, dialects, "\n\n");
}

const RUST_DOWN_DISPATCH_CHUNKS = (await renderDialectMap(
  "rust",
  "down_dispatch",
  RUST_DIALECT_ONLY_TOKENS,
)) as Record<string, string>;

export function rustDialectDownDispatchBlock(dialects: string[] = []): string {
  return filterChunks(RUST_DOWN_DISPATCH_CHUNKS, dialects, "\n");
}
