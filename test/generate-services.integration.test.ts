import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-services.ts";

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

const ROUTES_YAML = `routes:
  - getReport:
      method: GET
      path: /api/report
      service: ReportService
      function: run
  - health:
      method: GET
      path: /api/health
      service: HealthCheckService
      function: check
`;

const fixtureReader = (files: Record<string, string>) => memoryReader(files);

const textOf = (entries: GenerateEntry[], path: string): string => {
  const hit = entries.find((e) => e.kind === "content" && e.filename === path);
  assert.ok(hit, `missing entry ${path}`);
  assert.equal(hit.kind, "content");
  return hit.contents;
};

describe("generate-services", () => {
  it("emits generic DynamicService facade, custom stubs, and health check body", async () => {
    const entries = await generate({
      reader: fixtureReader({
        "types.yaml": TYPES,
        "datasource.yaml": DATASOURCE,
        "services.yaml": SERVICES_YAML,
        "routes.yaml": ROUTES_YAML,
      }),
      settings: {},
    });

    const paths = entries
      .map((e) => e.filename)
      .sort();
    assert.ok(paths.includes("userService.rs"), `got: ${paths.join(", ")}`);
    assert.ok(paths.includes("roleService.rs"));
    assert.ok(paths.includes("../custom/reportService.rs"));
    const healthPath = paths.find((p) => /health/i.test(p));
    assert.ok(healthPath, `health stub missing; got: ${paths.join(", ")}`);

    const user = textOf(entries, "userService.rs");
    assert.match(user, /pub struct UserService/);
    assert.match(user, /inner: Arc<dyn DynamicService>/);
    assert.match(user, /pub fn from_inner\(inner: Arc<dyn DynamicService>\)/);
    assert.match(user, /ctx\.entity_service\("user"\)/);
    assert.match(user, /impl DynamicService for UserService/);
    assert.match(user, /impl deterministic::routes::CrudService for UserService/);

    const report = textOf(entries, "../custom/reportService.rs");
    assert.match(report, /pub struct ReportService/);
    assert.match(report, /async fn run\(&self, _args: Value\)/);
    assert.match(report, /Ok\(serde_json::json!\(\{\}\)\)/);
    assert.match(report, /"run" => self\.run\(args\)\.await/);

    const health = textOf(entries, healthPath);
    assert.match(health, /use serde_json::\{json, Value\};/);
    assert.match(health, /async fn check\(&self, _args: Value\)/);
    assert.match(health, /Ok\(json!\(\{ "status": "ok" \}\)\)/);
  });

  it("rejects when services.yaml is missing", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: fixtureReader({
            "types.yaml": TYPES,
          }),
          settings: {},
        }),
      /services\.yaml/,
    );
  });
});
