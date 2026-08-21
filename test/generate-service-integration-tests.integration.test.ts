import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-service-integration-tests.ts";

const DS_YAML = `types:
  - user:
      fields:
        - email:
            type: string
            is_unique: true
  - role:
      datasource_type: readonly-lookup
      fields:
        - name:
            type: string
            is_unique: true
  - user_role:
      datasource_type: many-to-many
      fields:
        - user_id:
            type: number
            references: user.id
        - role_id:
            type: number
            references: role.id
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
    assert.match(body, /fn add_inserts_a_row_and_auto_populates_id_uuid_created_updated/);
    assert.match(body, /row\.get\("uuid"\)/);
    assert.match(body, /json!\(99999\)/);
  });

  it("emits nothing when datasource_types.yaml is absent", async () => {
    const entries = await generate({
      reader: memoryReader({
        "view_types.yaml": `types:
  - report:
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

  it("drops the uuid stamp assertion when datasource.id_type is uuid", async () => {
    const entries = await generate({
      reader: memoryReader(yaml),
      settings: { "datasource.id_type": "uuid" },
    });
    const body = textOf(entries, "userRoleServiceIntegrationTests.rs");
    assert.match(
      body,
      /fn add_inserts_a_row_and_auto_populates_id_created_updated/,
    );
    assert.doesNotMatch(body, /row\.get\("uuid"\)/);
    assert.match(
      body,
      /json!\("00000000-0000-0000-0000-000000000000"\)/,
    );
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
            "datasource_types.yaml": DS_YAML,
            "view_types.yaml": VIEW_YAML,
          }),
          settings: {},
        }),
      /services\.yaml/,
    );
  });
});
