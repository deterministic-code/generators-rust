import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "./common/deterministic-reader.ts";
import { generate } from "./generate-routes-tests.ts";

describe("generate-routes-tests", () => {
  it("emits nothing (retired lane)", async () => {
    const entries = await generate({
      reader: memoryReader({ "routes.yaml": "routes: []\n" }),
      settings: {},
    });
    assert.deepEqual(entries, []);
  });
});
