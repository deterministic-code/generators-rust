#!/usr/bin/env node
import { access, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runAsMain } from "./run-as-main.ts";
import { spawnAsync } from "./spawn-async.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..");

const DEFAULT_REGISTRY_NAME = "deterministic-code";
const DEFAULT_REGISTRY_INDEX_URL =
  "sparse+https://deterministic-code.com/registry/";

const USAGE = `Usage: publish-deterministic-crate.ts [options]

Publishes the deterministic Rust crate to the alternative registry by
delegating to \`cargo publish --registry <name>\`. CI invokes this on
crate-v* tag pushes; locally you can run it for the first bootstrap
publish.

Options:
  --version <ver>             Required in CI: must equal the version line
                              in rust/Cargo.toml; refuses on mismatch so a
                              tag named \`crate-v0.1.4\` can't publish a
                              0.1.3 crate. Omit for local use; the script
                              reads the manifest version directly.
  --manifest <path>           Path to the Cargo.toml to publish.
                              Default: rust/Cargo.toml under the repo root.
  --registry <name>           Cargo alternative-registry alias.
                              Default: deterministic-code.
  --index-url <url>           Sparse-index URL.
                              Default: sparse+https://deterministic-code.com/registry/
  --token <token>             Bearer token sent as Authorization on the
                              registry publish endpoint. If omitted, cargo
                              falls back to CARGO_REGISTRIES_<NAME>_TOKEN.
  --dry-run                   Pass --dry-run to cargo publish (packages
                              and validates without uploading).
  --print-only                Print the cargo invocation that would run,
                              exit 0 without spawning anything. Used by
                              unit tests; never used in CI.
  -h, --help                  Show this help.

Environment variables (cargo's own conventions):
  CARGO_REGISTRIES_<NAME>_TOKEN    Token fallback when --token is absent.
                                   <NAME> is the registry alias uppercased
                                   with hyphens replaced by underscores
                                   (deterministic-code → DETERMINISTIC_CODE).

Exit codes:
  0   Published (or dry-run completed) successfully.
  2   Argument validation failure (e.g. version mismatch, missing args).
  Non-zero passed through from cargo on publish failure.
`;

interface PublishArgs {
  version: string | null;
  manifest: string | null;
  registry: string;
  indexUrl: string;
  token: string | null;
  dryRun: boolean;
  printOnly: boolean;
  help?: boolean;
}

const VALUE_FLAGS: {
  flag: string;
  set: (args: PublishArgs, v: string) => void;
}[] = [
  { flag: "--version", set: (a, v) => (a.version = v) },
  { flag: "--manifest", set: (a, v) => (a.manifest = v) },
  { flag: "--registry", set: (a, v) => (a.registry = v) },
  { flag: "--index-url", set: (a, v) => (a.indexUrl = v) },
  { flag: "--token", set: (a, v) => (a.token = v) },
];

const BOOL_FLAGS: { flag: string; set: (args: PublishArgs) => void }[] = [
  { flag: "--dry-run", set: (a) => (a.dryRun = true) },
  { flag: "--print-only", set: (a) => (a.printOnly = true) },
];

function applyToken(args: PublishArgs, rest: string[], i: number): number {
  const a = rest[i];
  if (a === "-h" || a === "--help") {
    args.help = true;
    return i;
  }
  for (const { flag, set } of VALUE_FLAGS) {
    if (a === flag) {
      set(args, rest[++i]);
      return i;
    }
    if (a.startsWith(`${flag}=`)) {
      set(args, a.slice(flag.length + 1));
      return i;
    }
  }
  const bool = BOOL_FLAGS.find((b) => b.flag === a);
  if (!bool) throw new Error(`unknown argument: ${a}`);
  bool.set(args);
  return i;
}

export function parseArgs(argv: string[]): PublishArgs {
  const args: PublishArgs = {
    version: null,
    manifest: null,
    registry: DEFAULT_REGISTRY_NAME,
    indexUrl: DEFAULT_REGISTRY_INDEX_URL,
    token: null,
    dryRun: false,
    printOnly: false,
  };
  const rest = argv.slice(2);
  for (let i = 0; i < rest.length; i++) {
    i = applyToken(args, rest, i);
  }
  return args;
}

export async function readCargoVersion(manifestPath: string): Promise<string> {
  const text = await readFile(manifestPath, "utf8");
  const m = text.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!m) {
    throw new Error(
      `readCargoVersion: could not parse a top-level \`version = "..."\` from ${manifestPath}`,
    );
  }
  return m[1];
}

interface CargoInvocationArgs {
  manifestPath: string;
  registry: string;
  indexUrl: string;
  dryRun: boolean;
}

interface CargoInvocation {
  cmd: string;
  args: string[];
}

/** Build the cargo publish invocation (program + argv). Kept pure so unit
 *  tests can assert the exact command shape without spawning cargo. */
export function buildCargoInvocation({
  manifestPath,
  registry,
  indexUrl,
  dryRun,
}: CargoInvocationArgs): CargoInvocation {
  const cargoArgs = [
    "publish",
    "--manifest-path",
    manifestPath,
    "--registry",
    registry,
    "--config",
    `registries.${registry}.index = "${indexUrl}"`,
    "--no-verify",
  ];
  if (dryRun) cargoArgs.push("--dry-run");
  return { cmd: "cargo", args: cargoArgs };
}

/** Cargo's per-registry token env var name (e.g.
 *  CARGO_REGISTRIES_DETERMINISTIC_CODE_TOKEN). */
export function tokenEnvVar(registry: string): string {
  return `CARGO_REGISTRIES_${registry.toUpperCase().replace(/-/g, "_")}_TOKEN`;
}

function envFor(args: PublishArgs): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { ...process.env };
  if (args.token !== null) {
    env[tokenEnvVar(args.registry)] = args.token;
  }
  return env;
}

/** Resolve the manifest path, assert it exists, and reconcile its version with
 *  a `--version` arg (exits 2 on a missing manifest or version mismatch). */
async function resolveManifestAndVersion(
  args: PublishArgs,
): Promise<{ manifestPath: string; cargoVersion: string }> {
  const manifestPath = args.manifest
    ? resolve(args.manifest)
    : resolve(repoRoot, "rust", "Cargo.toml");
  const manifestExists = await access(manifestPath).then(
    () => true,
    () => false,
  );
  if (!manifestExists) {
    process.stderr.write(`manifest not found: ${manifestPath}\n`);
    process.exit(2);
  }
  const cargoVersion = await readCargoVersion(manifestPath);
  if (args.version !== null && args.version !== cargoVersion) {
    process.stderr.write(
      `--version mismatch: tag/arg says ${args.version}, Cargo.toml says ${cargoVersion}. ` +
        `Bump rust/Cargo.toml first, then tag.\n`,
    );
    process.exit(2);
  }
  return { manifestPath, cargoVersion };
}

async function runCargoPublish(
  invocation: CargoInvocation,
  args: PublishArgs,
  cargoVersion: string,
): Promise<void> {
  process.stdout.write(
    `publishing deterministic ${cargoVersion} → ${args.registry} (${args.indexUrl})${
      args.dryRun ? " [DRY RUN]" : ""
    }\n`,
  );
  const result = await spawnAsync(invocation.cmd, invocation.args, {
    stdio: "inherit",
    env: envFor(args),
  });
  if (result.code !== 0) {
    throw new Error(`cargo publish exited with code ${result.code}`);
  }
  process.stdout.write(
    `${args.dryRun ? "dry-run OK" : "published"}: deterministic ${cargoVersion}\n`,
  );
}

async function main(argv: string[]): Promise<void> {
  let args: PublishArgs;
  try {
    args = parseArgs(argv);
  } catch (e) {
    const err = e as Error;
    process.stderr.write(`${err.message}\n\n${USAGE}`);
    process.exit(2);
  }
  if (args.help) {
    process.stdout.write(USAGE);
    return;
  }
  const { manifestPath, cargoVersion } = await resolveManifestAndVersion(args);
  const invocation = buildCargoInvocation({
    manifestPath,
    registry: args.registry,
    indexUrl: args.indexUrl,
    dryRun: args.dryRun,
  });
  if (args.printOnly) {
    process.stdout.write(`${invocation.cmd} ${invocation.args.join(" ")}\n`);
    return;
  }
  await runCargoPublish(invocation, args, cargoVersion);
}

await runAsMain(import.meta, () => main(process.argv));
