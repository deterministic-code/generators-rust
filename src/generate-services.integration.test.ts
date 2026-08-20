import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "./generate-services.ts";

const DS_YAML = `types:
  - user:
      fields:
        - email:
            type: string
            is_unique: true
            size: 256
        - role_id:
            type: number
            references: role.id
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

const ROUTES_YAML = `routes:
  - getReport:
      method: GET
      path: /api/report
      service: ReportService
      serviceMethod: run
  - health:
      method: GET
      path: /api/health
      service: HealthCheckService
      serviceMethod: check
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
        "datasource_types.yaml": DS_YAML,
        "view_types.yaml": VIEW_YAML,
        "services.yaml": SERVICES_YAML,
        "routes.yaml": ROUTES_YAML,
      }),
      settings: {},
    });

    const paths = entries
      .map((e) => e.filename)
      .sort();
    assert.ok(paths.includes("user_service.rs"), `got: ${paths.join(", ")}`);
    assert.ok(paths.includes("role_service.rs"));
    assert.ok(paths.includes("../custom/report_service.rs"));
    assert.ok(paths.includes("../custom/health-check-service.rs"));

    const user = textOf(entries, "user_service.rs");
    assert.match(user, /pub struct UserService/);
    assert.match(user, /inner: Arc<dyn DynamicService>/);
    assert.match(user, /pub fn from_inner\(inner: Arc<dyn DynamicService>\)/);
    assert.match(user, /ctx\.entity_service\("user"\)/);
    assert.match(user, /impl DynamicService for UserService/);
    assert.match(user, /impl deterministic::routes::CrudService for UserService/);

    const report = textOf(entries, "../custom/report_service.rs");
    assert.match(report, /pub struct ReportService/);
    assert.match(report, /async fn run\(&self, _args: Value\)/);
    assert.match(report, /Ok\(serde_json::json!\(\{\}\)\)/);
    assert.match(report, /"run" => self\.run\(args\)\.await/);

    const health = textOf(entries, "../custom/health-check-service.rs");
    assert.match(health, /use serde_json::\{json, Value\};/);
    assert.match(health, /async fn check\(&self, _args: Value\)/);
    assert.match(health, /Ok\(json!\(\{ "status": "ok" \}\)\)/);
  });

  it("rejects when services.yaml is missing", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: fixtureReader({
            "datasource_types.yaml": DS_YAML,
            "view_types.yaml": VIEW_YAML,
          }),
          settings: {},
        }),
      /services\.yaml/,
    );
  });
});
