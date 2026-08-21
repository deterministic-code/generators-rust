import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, it } from "node:test";

const TEMPLATES = fileURLToPath(new URL("../src/templates", import.meta.url));
const TAG = /\{\{([#^/!>]|&)?([A-Za-z_][A-Za-z0-9_]*)\}\}/g;
const IDENT = /[A-Za-z0-9_]/;

const walkTmpls = async (dir: string): Promise<string[]> => {
  const out: string[] = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...(await walkTmpls(path)));
    else if (entry.name.endsWith(".tmpl")) out.push(path);
  }
  return out;
};

const glueHits = (text: string): string[] => {
  const hits: string[] = [];
  TAG.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = TAG.exec(text)) !== null) {
    const sigil = match[1];
    if (sigil !== undefined) continue;
    const before = match.index > 0 ? text[match.index - 1]! : "";
    const after = text[match.index + match[0].length] ?? "";
    if (IDENT.test(before) || IDENT.test(after)) {
      const start = Math.max(0, match.index - 24);
      const end = Math.min(text.length, match.index + match[0].length + 24);
      hits.push(text.slice(start, end).replace(/\s+/g, " "));
    }
  }
  return hits;
};

describe("mustache identifier audit", () => {
  it("does not squash identifier pieces around {{tokens}}", async () => {
    const files = await walkTmpls(TEMPLATES);
    const failures: string[] = [];
    for (const file of files) {
      const hits = glueHits(await readFile(file, "utf8"));
      for (const hit of hits) failures.push(`${file}: ${hit}`);
    }
    assert.deepEqual(failures, []);
  });
});
