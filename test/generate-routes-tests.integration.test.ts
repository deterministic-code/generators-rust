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
    assert.match(users, /use super::users::router;/);
    assert.match(
      users,
      /use crate::services::generated::userService::UserService;/,
    );
    assert.match(users, /fn post_user_returns_201/);
    assert.match(users, /uri\("\/api\/users\/99999"\)/);
    assert.match(users, /fn get_user_by_email_missing_returns_404/);
    assert.match(users, /uri\("\/api\/users\/email\/missing"\)/);
    assert.match(users, /fn get_user_by_role_id_returns_items/);
    assert.match(users, /uri\("\/api\/users\/role-id\/x"\)/);
    assert.match(users, /header\("if-match", "2020-01-01T00:00:00.000Z"\)/);
    assert.doesNotMatch(users, /get_user_by_slug/);

    const roles = textOf(entries, "rolesTests.rs");
    assert.match(roles, /fn get_role_list_returns_200/);
    assert.match(roles, /uri\("\/api\/roles"\)/);
    assert.doesNotMatch(roles, /post_role_returns_201/);

    const orders = textOf(entries, "ordersTests.rs");
    assert.match(orders, /header\("if-match", "2020-01-01T00:00:00.000Z"\)/);
    assert.match(orders, /fn put_order_missing_is_not_5xx/);
    assert.match(orders, /fn patch_order_missing_is_not_5xx/);
    assert.match(orders, /fn delete_order_missing_is_not_5xx/);
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
    assert.match(users, /header\("if-match", "2020-01-01T00:00:00.000Z"\)/);
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
    assert.match(users, /use super::usersRouter::router;/);
    assert.match(users, /use crate::features::user::userService::UserService;/);
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
