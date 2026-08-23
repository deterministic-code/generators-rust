import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { generate } from "../src/generate-perf-server.ts";

describe("generate-perf-server", () => {
  it("rejects a missing types.yaml", async () => {
    await assert.rejects(
      () => generate({ reader: memoryReader({}), settings: {} }),
      /types\.yaml is required/,
    );
  });

  it("emits the bin and a Cargo.toml PERF_BIN patch", async () => {
    const entries = await generate({
      reader: memoryReader({
        "types.yaml": `types:
  - user:
      tags: [datasource_type]
      fields:
        - email:
            type: string
`,
      }),
      settings: {},
    });
    assert.equal(entries[0]?.filename, "src/bin/perf_server.rs");
    assert.equal(entries[1]?.kind, "patch");
    if (entries[1]?.kind === "patch") {
      assert.equal(entries[1].section, "PERF_BIN");
      assert.match(entries[1].content, /perf_server/);
    }
  });
});
