import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-service-tests.ts";

const TYPES = `types:
  - user:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - email:
            type: string
            size: 256
  - role:
      tags: [datasource_type, view_type, readonly_lookup]
      inherits: set
      fields:
        - name:
            type: string
`;

const DATASOURCE = `includes:
  - types:
      filter: tag == "datasource_type"
types:
  - user:
      fields:
        - email:
            is_unique: true
  - role:
      fields:
        - name:
            is_unique: true
`;

const SERVICES_YAML = `includes:
  - types:
      filter: tag == "view_type"
services:
  - name: ReportService
`;

const yaml = {
  "types.yaml": TYPES,
  "datasource.yaml": DATASOURCE,
  "services.yaml": SERVICES_YAML,
};

const textOf = (entries: GenerateEntry[], path: string): string => {
  const hit = entries.find((e) => e.kind === "content" && e.filename === path);
  assert.ok(
    hit,
    `missing entry ${path}; got ${entries.map((e) => e.filename).join(", ")}`,
  );
  assert.equal(hit.kind, "content");
  return hit.contents;
};

describe("generate-service-tests", () => {
  it("emits a unit test per generic service and skips custom stubs", async () => {
    const entries = await generate({
      reader: memoryReader(yaml),
      settings: {},
    });
    const paths = entries.map((e) => e.filename).sort();
    assert.deepEqual(paths, ["roleServiceTests.rs", "userServiceTests.rs"]);

    const user = textOf(entries, "userServiceTests.rs");
    assert.match(user, /use super::userService::\*;/);
    assert.match(user, /fn service\(\) -> UserService/);
    assert.match(user, /UserService::from_inner/);
    assert.match(user, /InMemoryCrudRepository::new/);
    assert.match(user, /json!\(99999\)/);
    assert.match(user, /fn find_all_returns_empty_on_a_fresh_service/);
    assert.match(user, /fn add_then_find_round_trips/);
    assert.match(user, /fn invoke_find_all_delegates_to_the_inner_service/);
  });

  it("emits nothing without a types include", async () => {
    const entries = await generate({
      reader: memoryReader({
        ...yaml,
        "services.yaml": "services:\n  - name: ReportService\n",
      }),
      settings: {},
    });
    assert.deepEqual(entries, []);
  });

  it("nests tests under features/ when organize_by_feature is true", async () => {
    const entries = await generate({
      reader: memoryReader(yaml),
      settings: { "other.organize_by_feature": "true" },
    });
    const paths = entries.map((e) => e.filename).sort();
    assert.deepEqual(paths, [
      "features/role/__tests__/roleServiceTests.rs",
      "features/user/__tests__/userServiceTests.rs",
    ]);
  });

  it("rejects when services.yaml is missing", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({
            "types.yaml": TYPES,
          }),
          settings: {},
        }),
      /services\.yaml/,
    );
  });
});
