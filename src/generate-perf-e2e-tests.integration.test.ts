import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { generate } from "./generate-perf-e2e-tests.ts";

describe("generate-perf-e2e-tests", () => {
  it("emits the static rust client", async () => {
    const entries = await generate({
      reader: memoryReader({}),
      settings: {},
    });
    assert.equal(entries[0]?.filename, "app_perf_client.rs");
    assert.equal(entries[0]?.kind, "content");
    if (entries[0]?.kind === "content") {
      assert.match(entries[0].contents, /performance-plan\.yaml/);
    }
  });
});
