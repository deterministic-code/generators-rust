import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-service-integration-tests.ts";

const TYPES = `types:
  - user:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - email:
            type: string
  - role:
      tags: [datasource_type, view_type, readonly_lookup]
      inherits: set
      fields:
        - name:
            type: string
  - user_role:
      tags: [datasource_type, view_type, many_to_many]
      inherits: set
      fields:
        - user_id:
            type: number
            references: user.id
        - role_id:
            type: number
            references: role.id
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

describe("generate-service-integration-tests", () => {
  it("emits sqlite integration tests only for many-to-many services", async () => {
    const entries = await generate({
      reader: memoryReader(yaml),
      settings: {},
    });
    assert.deepEqual(
      entries.map((e) => e.filename),
      ["userRoleServiceIntegrationTests.rs"],
    );
    const body = textOf(entries, "userRoleServiceIntegrationTests.rs");
    assert.match(body, /use super::userRoleService::\*;/);
    assert.match(body, /fn service\(\) -> UserRoleService/);
    assert.match(body, /SqliteDatasourceOptions::in_memory/);
    assert.match(body, /CREATE TABLE user_role/);
    assert.match(body, /fn add_inserts_a_row_and_auto_populates_id_created_updated/);
    assert.match(body, /json!\(99999\)/);
  });

  it("emits nothing when types.yaml has no datasource types", async () => {
    const entries = await generate({
      reader: memoryReader({
        "types.yaml": `types:
  - report:
      tags: [view_type]
      fields:
        - title:
            type: string
`,
        "services.yaml": SERVICES_YAML,
      }),
      settings: {},
    });
    assert.deepEqual(entries, []);
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
    assert.deepEqual(
      entries.map((e) => e.filename),
      ["features/userRole/__tests__/userRoleServiceIntegrationTests.rs"],
    );
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
