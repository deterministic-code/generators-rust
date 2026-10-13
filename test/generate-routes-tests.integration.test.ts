import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-routes-tests.ts";

const TYPES = `types:
  - user:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - email:
            type: string
        - role_id:
            type: number
            references: role.id
  - role:
      tags: [datasource_type, view_type, readonly_lookup]
      inherits: set
      fields:
        - name:
            type: string
  - order:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - label:
            type: string
  - sku:
      tags: [datasource_type, view_type]
      fields:
        - code:
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
  - order:
      use_optimistic_concurrency: true
  - sku:
      fields:
        - code:
            is_fixed_id: true
`;

const ROUTES_YAML = `includes:
  - types:
      filter: 'tag == "view_type"'
routes:
  - get_users_by_email:
  - users_by_role_id:
      entity: user
      byField: role_id
  - users_by_slug:
      entity: user
      byField: slug
      methods:
        - PUT
`;

const yaml = {
  "types.yaml": TYPES,
  "datasource.yaml": DATASOURCE,
  "routes.yaml": ROUTES_YAML,
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

describe("generate-routes-tests", () => {
  it("emits CRUD, readonly, by-field GET, and OCC router tests", async () => {
    const entries = await generate({
      reader: memoryReader(yaml),
      settings: {},
    });
    const paths = entries.map((e) => e.filename).sort();
    assert.deepEqual(paths, [
      "ordersTests.rs",
      "rolesTests.rs",
      "skusTests.rs",
      "usersTests.rs",
    ]);

    const users = textOf(entries, "usersTests.rs");
    assert.match(users, /use super::users::\*;/);
    assert.match(users, /fn router_is_defined/);
    assert.doesNotMatch(users, /deterministic::/);

    const roles = textOf(entries, "rolesTests.rs");
    assert.match(roles, /fn router_is_defined/);
    assert.doesNotMatch(roles, /post_role_returns_201/);

    const orders = textOf(entries, "ordersTests.rs");
    assert.match(orders, /fn router_is_defined/);
    assert.doesNotMatch(orders, /if-match/);
  });

  it("emits nothing without a types include", async () => {
    const entries = await generate({
      reader: memoryReader({
        ...yaml,
        "routes.yaml": "routes: []\n",
      }),
      settings: {},
    });
    assert.deepEqual(entries, []);
  });

  it("emits OCC headers when enabled globally", async () => {
    const entries = await generate({
      reader: memoryReader({
        ...yaml,
        "routes.yaml": `includes:
  - types:
      filter: 'type == "user"'
routes: []
`,
      }),
      settings: { "datasource.use_optimistic_concurrency": "true" },
    });
    const users = textOf(entries, "usersTests.rs");
    assert.match(users, /fn router_is_defined/);
    assert.doesNotMatch(users, /if-match/);
  });

  it("nests tests under features/ when organize_by_feature is true", async () => {
    const entries = await generate({
      reader: memoryReader({
        ...yaml,
        "routes.yaml": `includes:
  - types:
      filter: 'type == "user"'
routes: []
`,
      }),
      settings: { "other.organize_by_feature": "true" },
    });
    assert.deepEqual(
      entries.map((e) => e.filename),
      ["features/user/__tests__/usersRouterTests.rs"],
    );
    const users = textOf(entries, "features/user/__tests__/usersRouterTests.rs");
    assert.match(users, /use super::usersRouter::\*;/);
    assert.match(users, /fn router_is_defined/);
  });

  it("rejects when routes.yaml is missing", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({
            "types.yaml": TYPES,
          }),
          settings: {},
        }),
      /routes\.yaml/,
    );
  });
});
