import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { pascalCase } from "change-case";
import { parseDatasourceTypes } from "./parse-datasource-types.ts";
import { parseViewTypes } from "./parse-view-types.ts";
import { parseServices } from "./parse-services.ts";

const serviceClassName = (entity: string) => pascalCase(`${entity}_service`);

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
types:
  - user_summary:
      inherits: datasource_types.user
      omit:
        - role_id
`;

const SERVICES_YAML = `includes:
  - view_type_services:
      filter: 'type is view_type || type is datasource_type'
services:
  - name: ReportService
    module: ./services/custom/report-service
`;

const ROUTES_YAML = `routes:
  - getReport:
      method: GET
      path: /api/report
      service: ReportService
      serviceMethod: run
`;

describe("parseServices", () => {
  it("seeds HealthCheckService, builds generics, and wires custom methods", () => {
    const datasources = parseDatasourceTypes({ yaml: DS_YAML, idType: "integer" });
    const views = parseViewTypes({ viewYaml: VIEW_YAML, datasourceYaml: DS_YAML });
    const parsed = parseServices({
      servicesYaml: SERVICES_YAML,
      views,
      datasources,
      routesYaml: ROUTES_YAML,
      serviceClassName,
    });

    assert.equal(parsed.customs[0]?.name, "HealthCheckService");
    assert.deepEqual(
      parsed.customs.map((c) => c.name),
      ["HealthCheckService", "ReportService"],
    );
    assert.deepEqual(parsed.customs[1]?.methods, ["run"]);

    const names = parsed.generics.map((g) => g.name).sort();
    assert.ok(names.includes("user"));
    assert.ok(names.includes("role"));
    assert.ok(names.includes("user_summary"));

    const user = parsed.generics.find((g) => g.name === "user");
    assert.ok(user);
    assert.equal(user.kind, "view_type");
    assert.deepEqual(
      user.byFields.map((f) => f.field),
      ["email"],
    );
  });

  it("suppresses a generic when a custom stub uses the same class name", () => {
    const datasources = parseDatasourceTypes({ yaml: DS_YAML, idType: "integer" });
    const views = parseViewTypes({ viewYaml: VIEW_YAML, datasourceYaml: DS_YAML });
    const parsed = parseServices({
      servicesYaml: `includes:
  - view_type_services:
      filter: 'type == "user"'
services:
  - name: UserService
    module: ./services/custom/user-service
`,
      views,
      datasources,
      serviceClassName,
    });
    assert.equal(parsed.generics.length, 0);
    assert.ok(parsed.customs.some((c) => c.name === "UserService"));
  });
});
