import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "./generate-service-tests.ts";

const DS_YAML = `types:
  - user:
      fields:
        - email:
            type: string
            is_unique: true
            size: 256
  - role:
      datasource_type: readonly-lookup
      fields:
        - name:
            type: string
            is_unique: true
`;

const VIEW_YAML = `includes:
  - datasource_types:
      include: "*"
types: []
`;

const SERVICES_YAML = `includes:
  - view_type_services:
      filter: 'type is view_type'
services:
  - name: ReportService
`;

const yaml = {
  "datasource_types.yaml": DS_YAML,
  "view_types.yaml": VIEW_YAML,
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
    assert.deepEqual(paths, ["role_service_tests.rs", "user_service_tests.rs"]);

    const user = textOf(entries, "user_service_tests.rs");
    assert.match(user, /use super::user_service::\*;/);
    assert.match(user, /fn service\(\) -> UserService/);
    assert.match(user, /UserService::from_inner/);
    assert.match(user, /InMemoryCrudRepository::new/);
    assert.match(user, /json!\(99999\)/);
    assert.match(user, /fn find_all_returns_empty_on_a_fresh_service/);
    assert.match(user, /fn add_then_find_round_trips/);
    assert.match(user, /fn invoke_find_all_delegates_to_the_inner_service/);
  });

  it("emits nothing without view_type_services", async () => {
    const entries = await generate({
      reader: memoryReader({
        ...yaml,
        "services.yaml": "services:\n  - name: ReportService\n",
      }),
      settings: {},
    });
    assert.deepEqual(entries, []);
  });

  it("uses a uuid missing id when datasource.id_type is uuid", async () => {
    const entries = await generate({
      reader: memoryReader(yaml),
      settings: { "datasource.id_type": "uuid" },
    });
    const user = textOf(entries, "user_service_tests.rs");
    assert.match(
      user,
      /json!\("00000000-0000-0000-0000-000000000000"\)/,
    );
    assert.doesNotMatch(user, /json!\(99999\)/);
  });

  it("nests tests under features/ when organize_by_feature is true", async () => {
    const entries = await generate({
      reader: memoryReader(yaml),
      settings: { "other.organize_by_feature": "true" },
    });
    const paths = entries.map((e) => e.filename).sort();
    assert.deepEqual(paths, [
      "features/role/__tests__/role_service_tests.rs",
      "features/user/__tests__/user_service_tests.rs",
    ]);
  });

  it("rejects when services.yaml is missing", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({
            "datasource_types.yaml": DS_YAML,
            "view_types.yaml": VIEW_YAML,
          }),
          settings: {},
        }),
      /services\.yaml/,
    );
  });
});
