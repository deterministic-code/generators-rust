import { spawn } from "node:child_process";
import type { SpawnOptions } from "node:child_process";

interface SpawnAsyncOptions extends SpawnOptions {
  encoding?: BufferEncoding;
}

interface SpawnAsyncResult {
  code: number;
  stdout: string | undefined;
  stderr: string | undefined;
}

export function spawnAsync(
  cmd: string,
  args: readonly string[],
  opts: SpawnAsyncOptions = {},
): Promise<SpawnAsyncResult> {
  return new Promise((resolveOuter, reject) => {
    const child = spawn(cmd, args, opts);
    let stdout: string | undefined;
    let stderr: string | undefined;
    if (child.stdout) {
      stdout = "";
      child.stdout.setEncoding(opts.encoding ?? "utf8");
      child.stdout.on("data", (chunk: string) => {
        stdout += chunk;
      });
    }
    if (child.stderr) {
      stderr = "";
      child.stderr.setEncoding(opts.encoding ?? "utf8");
      child.stderr.on("data", (chunk: string) => {
        stderr += chunk;
      });
    }
    child.on("error", reject);
    child.on("close", (code) =>
      resolveOuter({ code: code ?? 1, stdout, stderr }),
    );
  });
}
