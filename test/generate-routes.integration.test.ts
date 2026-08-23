import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-routes.ts";

const TYPES = `types:
  - user:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - email:
            type: string
            size: 256
        - role_id:
            type: number
            references: role.id
        - active:
            type: boolean
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
  - order_item:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - order_id:
            type: number
            references: order.id
        - sku:
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
`;

const ROUTES_YAML = `includes:
  - types:
      filter: 'tag == "view_type"'
routes:
  - users_by_email:
combined_routes:
  - order:
      combines:
        - order_item
`;

const textOf = (entries: GenerateEntry[], path: string): string => {
  const hit = entries.find((e) => e.kind === "content" && e.filename === path);
  assert.ok(
    hit,
    `missing entry ${path}; got ${entries.map((e) => e.filename).join(", ")}`,
  );
  assert.equal(hit.kind, "content");
  return hit.contents;
};

describe("generate-routes", () => {
  it("emits CRUD, readonly, byField GET, validators, coercion, and app wiring", async () => {
    const entries = await generate({
      reader: memoryReader({
        "types.yaml": TYPES,
        "datasource.yaml": DATASOURCE,
        "routes.yaml": ROUTES_YAML,
      }),
      settings: {},
    });

    const paths = entries.map((e) => e.filename).sort();
    assert.ok(paths.includes("users.rs"), `got: ${paths.join(", ")}`);
    assert.ok(paths.includes("roles.rs"));
    assert.ok(paths.includes("orders.rs"));
    assert.ok(paths.includes("app_wiring.rs"));
    assert.ok(
      !paths.some((p) => p.includes("health") || p.includes("get_health")),
      `custom health must not emit a route file; got ${paths.join(", ")}`,
    );
    assert.ok(
      !paths.some((p) => p.includes("nested")),
      `nested routers must not emit; got ${paths.join(", ")}`,
    );

    const users = textOf(entries, "users.rs");
    assert.match(users, /create_crud_router/);
    assert.match(users, /validate_create_user/);
    assert.match(users, /validate_update_user/);
    assert.match(users, /create_by_field_router/);
    assert.match(users, /field: "email"/);
    assert.match(users, /ByFieldMethod::Get/);
    assert.match(users, /coerce_row_types/);
    assert.match(users, /use crate::services::generated::userService::UserService;/);

    const roles = textOf(entries, "roles.rs");
    assert.match(roles, /create_read_only_router/);
    assert.match(roles, /entity_name: "role"/);

    const orders = textOf(entries, "orders.rs");
    assert.match(orders, /use_optimistic_concurrency: true/);
    assert.match(orders, /order_items/);

    const wiring = textOf(entries, "app_wiring.rs");
    assert.match(wiring, /pub fn compose_router/);
    assert.match(wiring, /UserService::from_context/);
    assert.match(wiring, /crate::routes::generated::users::router/);
    assert.match(wiring, /crate::routes::generated::roles::router/);
  });

  it("emits OCC when enabled globally or per entity", async () => {
    const entries = await generate({
      reader: memoryReader({
        "types.yaml": TYPES,
        "datasource.yaml": DATASOURCE,
        "routes.yaml": `includes:
  - types:
      filter: 'type == "order" || type == "user"'
routes: []
`,
      }),
      settings: { "datasource.use_optimistic_concurrency": "true" },
    });
    const orders = textOf(entries, "orders.rs");
    assert.match(orders, /use_optimistic_concurrency: true/);
    const users = textOf(entries, "users.rs");
    assert.match(users, /use_optimistic_concurrency: true/);
  });

  it("rejects missing routes.yaml", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({
            "types.yaml": TYPES,
            "datasource.yaml": DATASOURCE,
          }),
          settings: {},
        }),
      /routes\.yaml/,
    );
  });
});
