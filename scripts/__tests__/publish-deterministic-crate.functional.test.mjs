import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import {
  buildCargoInvocation,
  parseArgs,
  readCargoVersion,
  tokenEnvVar,
} from "../publish-deterministic-crate.ts";

const execFileAsync = promisify(execFile);
const __dirname = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = resolve(__dirname, "..", "..");
const cli = join(repoRoot, "scripts", "publish-deterministic-crate.ts");

async function runCli(args, opts = {}) {
  try {
    const { stdout, stderr } = await execFileAsync(
      process.execPath,
      [cli, ...args],
      { encoding: "utf8", cwd: repoRoot, env: { ...process.env, ...opts.env } },
    );
    return { stdout, stderr, status: 0 };
  } catch (e) {
    return {
      stdout: e.stdout?.toString() ?? "",
      stderr: e.stderr?.toString() ?? "",
      status: e.code ?? 1,
    };
  }
}

describe("parseArgs — defaults + flag parsing", () => {
  test("no args yields the documented defaults", () => {
    const a = parseArgs(["node", "script"]);
    expect(a.version).toBeNull();
    expect(a.manifest).toBeNull();
    expect(a.registry).toBe("deterministic-code");
    expect(a.indexUrl).toBe("sparse+https://deterministic-code.com/registry/");
    expect(a.token).toBeNull();
    expect(a.dryRun).toBe(false);
    expect(a.printOnly).toBe(false);
  });

  test("--flag value form", () => {
    const a = parseArgs([
      "node",
      "script",
      "--version",
      "1.2.3",
      "--registry",
      "internal",
      "--token",
      "abc",
      "--dry-run",
      "--print-only",
    ]);
    expect(a.version).toBe("1.2.3");
    expect(a.registry).toBe("internal");
    expect(a.token).toBe("abc");
    expect(a.dryRun).toBe(true);
    expect(a.printOnly).toBe(true);
  });

  test("--flag=value form", () => {
    const a = parseArgs(["node", "script", "--version=0.5.0"]);
    expect(a.version).toBe("0.5.0");
  });

  test("unknown flag throws (no silent acceptance)", () => {
    expect(() => parseArgs(["node", "script", "--bogus"])).toThrow(
      /unknown argument: --bogus/,
    );
  });
});

describe("tokenEnvVar — cargo's per-registry convention", () => {
  test("uppercases + hyphen → underscore (deterministic-code → DETERMINISTIC_CODE)", () => {
    expect(tokenEnvVar("deterministic-code")).toBe(
      "CARGO_REGISTRIES_DETERMINISTIC_CODE_TOKEN",
    );
    expect(tokenEnvVar("internal")).toBe("CARGO_REGISTRIES_INTERNAL_TOKEN");
  });
});

describe("buildCargoInvocation — wire-compatible cargo publish command", () => {
  test("default (no dry-run) emits the publish-only argv shape cargo expects", () => {
    const inv = buildCargoInvocation({
      manifestPath: "/path/to/Cargo.toml",
      registry: "deterministic-code",
      indexUrl: "sparse+https://example.test/registry/",
      dryRun: false,
    });
    expect(inv.cmd).toBe("cargo");
    expect(inv.args).toEqual([
      "publish",
      "--manifest-path",
      "/path/to/Cargo.toml",
      "--registry",
      "deterministic-code",
      "--config",
      `registries.deterministic-code.index = "sparse+https://example.test/registry/"`,
      "--no-verify",
    ]);
  });

  test("dryRun=true appends --dry-run as the final arg", () => {
    const inv = buildCargoInvocation({
      manifestPath: "/m",
      registry: "r",
      indexUrl: "i",
      dryRun: true,
    });
    expect(inv.args[inv.args.length - 1]).toBe("--dry-run");
  });
});

describe('readCargoVersion — pulls `version = "X.Y.Z"` from the top-level [package] block', () => {
  test("reads the version line from a real-shaped Cargo.toml", async () => {
    const dir = await mkdtemp(join(tmpdir(), "publish-crate-version-"));
    try {
      const path = join(dir, "Cargo.toml");
      await writeFile(
        path,
        `[package]\nname = "deterministic"\nversion = "0.1.4"\n`,
      );
      await expect(readCargoVersion(path)).resolves.toBe("0.1.4");
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  test("throws when no version line is present (refuses to publish an unversioned crate)", async () => {
    const dir = await mkdtemp(join(tmpdir(), "publish-crate-no-version-"));
    try {
      const path = join(dir, "Cargo.toml");
      await writeFile(path, `[package]\nname = "deterministic"\n`);
      await expect(readCargoVersion(path)).rejects.toThrow(/could not parse/);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("CLI — end-to-end (subprocess) on a fixture manifest", () => {
  test("--help exits 0 and prints USAGE", async () => {
    const r = await runCli(["--help"]);
    expect(r.status).toBe(0);
    expect(r.stdout).toMatch(/Usage:/);
    expect(r.stdout).toMatch(/--dry-run/);
  });

  test("--print-only with --manifest pointing at a fixture prints the exact cargo invocation, no spawn", async () => {
    const dir = await mkdtemp(join(tmpdir(), "publish-crate-printonly-"));
    try {
      const manifestPath = join(dir, "Cargo.toml");
      await writeFile(
        manifestPath,
        `[package]\nname = "deterministic"\nversion = "0.1.4"\n`,
      );
      const r = await runCli([
        "--manifest",
        manifestPath,
        "--registry",
        "internal",
        "--index-url",
        "sparse+https://example.test/i/",
        "--print-only",
      ]);
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("cargo publish");
      expect(r.stdout).toContain("--manifest-path");
      expect(r.stdout).toContain(manifestPath);
      expect(r.stdout).toContain("--registry internal");
      expect(r.stdout).toContain(
        `registries.internal.index = "sparse+https://example.test/i/"`,
      );
      expect(r.stdout).toContain("--no-verify");
      expect(r.stdout).not.toContain("--dry-run");
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  test("--version mismatch ⇒ exit 2 with a message naming both versions (so CI tag-vs-Cargo.toml drift is obvious in the logs)", async () => {
    const dir = await mkdtemp(join(tmpdir(), "publish-crate-mismatch-"));
    try {
      const manifestPath = join(dir, "Cargo.toml");
      await writeFile(
        manifestPath,
        `[package]\nname = "deterministic"\nversion = "0.1.4"\n`,
      );
      const r = await runCli([
        "--manifest",
        manifestPath,
        "--version",
        "9.9.9",
        "--print-only",
      ]);
      expect(r.status).toBe(2);
      expect(r.stderr).toMatch(/mismatch/);
      expect(r.stderr).toMatch(/9\.9\.9/);
      expect(r.stderr).toMatch(/0\.1\.4/);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  test("--version matching Cargo.toml is accepted (print-only path)", async () => {
    const dir = await mkdtemp(join(tmpdir(), "publish-crate-match-"));
    try {
      const manifestPath = join(dir, "Cargo.toml");
      await writeFile(
        manifestPath,
        `[package]\nname = "deterministic"\nversion = "0.1.4"\n`,
      );
      const r = await runCli([
        "--manifest",
        manifestPath,
        "--version",
        "0.1.4",
        "--print-only",
      ]);
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("cargo publish");
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  test("missing manifest ⇒ exit 2 (don't silently fall through to cargo)", async () => {
    const r = await runCli([
      "--manifest",
      "/nonexistent/Cargo.toml",
      "--print-only",
    ]);
    expect(r.status).toBe(2);
    expect(r.stderr).toMatch(/manifest not found/);
  });

  test("unknown flag ⇒ exit 2 with USAGE", async () => {
    const r = await runCli(["--bogus"]);
    expect(r.status).toBe(2);
    expect(r.stderr).toMatch(/unknown argument/);
    expect(r.stderr).toMatch(/Usage:/);
  });
});
