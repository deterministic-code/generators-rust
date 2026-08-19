import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { generate } from "./generate-routes-e2e-test.ts";

describe("generate-routes-e2e-test", () => {
  it("emits nothing when datasource_types.yaml is absent", async () => {
    const entries = await generate({
      reader: memoryReader({}),
      settings: {},
    });
    assert.deepEqual(entries, []);
  });

  it("emits an in-memory router e2e file for a regular table", async () => {
    const entries = await generate({
      reader: memoryReader({
        "datasource_types.yaml": `types:
  - contact:
      fields:
        - name: { type: string }
`,
      }),
      settings: {},
    });
    assert.equal(entries.length, 1);
    assert.equal(entries[0]?.kind, "content");
    assert.equal(entries[0]?.filename, "app_routes_e2e.rs");
    if (entries[0]?.kind === "content") {
      assert.match(entries[0].contents, /contact_list_returns_200/);
    }
  });
});
