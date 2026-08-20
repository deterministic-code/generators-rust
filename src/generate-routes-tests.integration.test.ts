import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "./generate-routes-tests.ts";

const DS_YAML = `types:
  - user:
      fields:
        - email:
            type: string
            is_unique: true
        - role_id:
            type: number
            references: role.id
  - role:
      datasource_type: readonly-lookup
      fields:
        - name:
            type: string
            is_unique: true
  - order:
      use_optimistic_concurrency: true
      fields:
        - label:
            type: string
  - sku:
      fields:
        - code:
            type: string
            primary_key: true
`;

const VIEW_YAML = `includes:
  - datasource_types:
      include: "*"
types: []
`;

const ROUTES_YAML = `includes:
  - view_type_routes:
      filter: 'type is view_type || type is datasource_type'
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
  "datasource_types.yaml": DS_YAML,
  "view_types.yaml": VIEW_YAML,
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
      "orders_tests.rs",
      "roles_tests.rs",
      "skus_tests.rs",
      "users_tests.rs",
    ]);

    const users = textOf(entries, "users_tests.rs");
    assert.match(users, /use super::users::router;/);
    assert.match(
      users,
      /use crate::services::generated::user_service::UserService;/,
    );
    assert.match(users, /fn post_user_returns_201/);
    assert.match(users, /uri\("\/api\/users\/99999"\)/);
    assert.match(users, /fn get_user_by_email_missing_returns_404/);
    assert.match(users, /uri\("\/api\/users\/email\/missing"\)/);
    assert.match(users, /fn get_user_by_role_id_returns_items/);
    assert.match(users, /uri\("\/api\/users\/role-id\/x"\)/);
    assert.doesNotMatch(users, /if-match/);
    assert.doesNotMatch(users, /get_user_by_slug/);

    const roles = textOf(entries, "roles_tests.rs");
    assert.match(roles, /fn get_role_list_returns_200/);
    assert.match(roles, /uri\("\/api\/roles"\)/);
    assert.doesNotMatch(roles, /post_role_returns_201/);

    const orders = textOf(entries, "orders_tests.rs");
    assert.match(orders, /header\("if-match", "2020-01-01T00:00:00.000Z"\)/);
    assert.match(orders, /fn put_order_missing_is_not_5xx/);
    assert.match(orders, /fn patch_order_missing_is_not_5xx/);
    assert.match(orders, /fn delete_order_missing_is_not_5xx/);
  });

  it("emits nothing without view_type_routes", async () => {
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
  - view_type_routes:
      filter: 'type == "user"'
routes: []
`,
      }),
      settings: { "datasource.use_optimistic_concurrency": "true" },
    });
    const users = textOf(entries, "users_tests.rs");
    assert.match(users, /header\("if-match", "2020-01-01T00:00:00.000Z"\)/);
  });

  it("uses a uuid missing id when datasource.id_type is uuid", async () => {
    const entries = await generate({
      reader: memoryReader(yaml),
      settings: { "datasource.id_type": "uuid" },
    });
    const users = textOf(entries, "users_tests.rs");
    assert.match(
      users,
      /uri\("\/api\/users\/00000000-0000-0000-0000-000000000000"\)/,
    );
  });

  it("nests tests under features/ when organize_by_feature is true", async () => {
    const entries = await generate({
      reader: memoryReader({
        ...yaml,
        "routes.yaml": `includes:
  - view_type_routes:
      filter: 'type == "user"'
routes: []
`,
      }),
      settings: { "other.organize_by_feature": "true" },
    });
    assert.deepEqual(
      entries.map((e) => e.filename),
      ["features/user/__tests__/users_router_tests.rs"],
    );
    const users = textOf(entries, "features/user/__tests__/users_router_tests.rs");
    assert.match(users, /use super::users_router::router;/);
    assert.match(users, /use crate::features::user::user_service::UserService;/);
  });

  it("rejects when routes.yaml is missing", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({
            "datasource_types.yaml": DS_YAML,
            "view_types.yaml": VIEW_YAML,
          }),
          settings: {},
        }),
      /routes\.yaml/,
    );
  });
});
