import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { generate } from "./generate-service-integration-tests.ts";

describe("generate-service-integration-tests", () => {
  it("emits nothing (retired lane)", async () => {
    const entries = await generate({
      reader: memoryReader({ "services.yaml": "services: []\n" }),
      settings: {},
    });
    assert.deepEqual(entries, []);
  });
});
