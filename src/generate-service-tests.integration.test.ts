import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { generate } from "./generate-service-tests.ts";

describe("generate-service-tests", () => {
  it("emits nothing (retired lane)", async () => {
    const entries = await generate({
      reader: memoryReader({
        "services.yaml": `includes:
  - view_type_services:
      filter: 'type is view_type'
services: []
`,
      }),
      settings: {},
    });
    assert.deepEqual(entries, []);
  });
});
