import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { parseDatasourceTypes } from "./parse-datasource-types.ts";
import { parseViewTypes } from "./parse-view-types.ts";
import {
  entityUsesOptimisticConcurrency,
  parseRoutes,
} from "./parse-routes.ts";

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
  - project:
      fields:
        - name:
            type: string
  - project_setting:
      fields:
        - project_id:
            type: number
            references: project.id
        - key:
            type: string
  - internal_sink:
      target: None
      fields:
        - label:
            type: string
  - widget:
      datasource_type: many-to-many
      use_optimistic_concurrency: true
      fields:
        - name:
            type: string
  - app:
      use_optimistic_concurrency: false
      fields:
        - name:
            type: string
`;

const VIEW_YAML = `includes:
  - datasource_types:
      include: "*"
types:
  - user_summary:
      inherits: datasource_types.user
      omit:
        - role_id
  - search_result:
      one_of:
        - user
        - role
`;

const parseFixture = (routesYaml: string) => {
  const datasources = parseDatasourceTypes({ yaml: DS_YAML, idType: "integer" });
  const views = parseViewTypes({ viewYaml: VIEW_YAML, datasourceYaml: DS_YAML });
  return parseRoutes({ routesYaml, views, datasources });
};

describe("parseRoutes", () => {
  it("seeds GET /api/health as the first custom route when missing", () => {
    const parsed = parseFixture(`includes:
  - view_type_routes:
      filter: 'type inherits datasource_types'
routes: []`);

    assert.equal(parsed.customs[0]?.name, "getHealth");
    assert.equal(
      (parsed.customs[0]?.entry.getHealth as { path?: string })?.path,
      "/api/health",
    );
  });

  it("returns empty candidates when view_type_routes is absent", () => {
    const parsed = parseFixture(`routes:
  - getReport:
      method: GET
      path: /api/report
      service: ReportService
      serviceMethod: run`);

    assert.equal(parsed.candidates.length, 0);
    assert.ok(parsed.customs.some((c) => c.name === "getReport"));
  });

  it("filters survivors by view_type_routes and drops target None", () => {
    const parsed = parseFixture(`includes:
  - view_type_routes:
      filter: 'type inherits datasource_types'
routes: []`);

    const names = parsed.candidates.map((c) => c.name).sort();
    assert.ok(names.includes("user"));
    assert.ok(names.includes("role"));
    assert.ok(names.includes("project"));
    assert.ok(names.includes("user_summary"));
    assert.ok(!names.includes("internal_sink"));
    assert.ok(!names.includes("search_result"));
  });

  it("attaches byField routes from shorthand and verbose entries", () => {
    const parsed = parseFixture(`includes:
  - view_type_routes:
      filter: 'type == "user"'
routes:
  - get_users_by_email:
  - users_by_slug:
      entity: user
      byField: slug
      methods:
        - GET
  - getReport:
      method: GET
      path: /api/report
      service: ReportService
      serviceMethod: run`);

    const user = parsed.candidates.find((c) => c.name === "user");
    assert.ok(user);
    assert.deepEqual(
      user.byFields.map((b) => b.byField).sort(),
      ["email", "slug"],
    );
    assert.equal(user.byFields.find((b) => b.byField === "email")?.byFieldUnique, true);
    assert.deepEqual(
      user.byFields.find((b) => b.byField === "email")?.methods,
      ["GET"],
    );
    assert.deepEqual(
      user.byFields.find((b) => b.byField === "slug")?.methods,
      ["GET"],
    );
  });

  it("splits custom routes from by-field entries", () => {
    const parsed = parseFixture(`includes:
  - view_type_routes:
      filter: 'type inherits datasource_types'
routes:
  - get_users_by_email:
  - getReport:
      method: GET
      path: /api/report
      service: ReportService
      serviceMethod: run`);

    assert.ok(parsed.customs.some((c) => c.name === "getHealth"));
    assert.ok(parsed.customs.some((c) => c.name === "getReport"));
    assert.ok(!parsed.customs.some((c) => c.name === "get_users_by_email"));
  });

  it("excludes direct-FK combined children from top-level candidates", () => {
    const parsed = parseFixture(`includes:
  - view_type_routes:
      filter: 'type inherits datasource_types'
combined_routes:
  - project:
      combined_types:
        - project_setting
routes: []`);

    assert.ok(parsed.childrenOnly.has("project_setting"));
    assert.ok(!parsed.candidates.some((c) => c.name === "project_setting"));
    assert.ok(parsed.candidates.some((c) => c.name === "project"));
  });

  it("collects direct-fk nested descriptors with default parent paths", () => {
    const parsed = parseFixture(`includes:
  - view_type_routes:
      filter: 'type inherits datasource_types'
combined_routes:
  - project:
      combined_types:
        - project_setting
routes: []`);

    assert.equal(parsed.nested.length, 1);
    const desc = parsed.nested[0];
    assert.equal(desc?.kind, "direct-fk");
    if (desc?.kind !== "direct-fk") return;
    assert.equal(desc.parent, "project");
    assert.equal(desc.parentParam, "projectId");
    assert.equal(desc.parentBasePath, "/api/projects/:projectId");
    assert.equal(desc.child.name, "project_setting");
    assert.equal(desc.fkColumn, "project_id");
    assert.equal(desc.segment, "/project-settings");
    assert.equal(desc.segmentTail, "project-settings");
  });

  it("does not mark m2m via/target children as childrenOnly", () => {
    const m2mDs = `types:
  - organization:
      fields: []
  - tag:
      fields:
        - name:
            type: string
  - org_tag:
      datasource_type: many-to-many
      fields:
        - organization_id:
            type: number
            references: organization.id
        - tag_id:
            type: number
            references: tag.id
`;
    const m2mView = `includes:
  - datasource_types:
      include: "*"
types: []`;
    const datasources = parseDatasourceTypes({ yaml: m2mDs, idType: "integer" });
    const views = parseViewTypes({ viewYaml: m2mView, datasourceYaml: m2mDs });
    const parsed = parseRoutes({
      routesYaml: `includes:
  - view_type_routes:
      filter: 'type inherits datasource_types'
combined_routes:
  - organization:
      combined_types:
        - tag:
            via: org_tag
            target: tag
routes: []`,
      views,
      datasources,
    });

    assert.ok(!parsed.childrenOnly.has("tag"));
  });
});

describe("entityUsesOptimisticConcurrency", () => {
  it("returns false for junction and readonly-lookup tables", () => {
    assert.equal(
      entityUsesOptimisticConcurrency(
        { datasourceType: "many-to-many" },
        true,
      ),
      false,
    );
    assert.equal(
      entityUsesOptimisticConcurrency(
        { datasourceType: "readonly-lookup" },
        true,
      ),
      false,
    );
  });

  it("prefers explicit per-type flag over the global default", () => {
    assert.equal(
      entityUsesOptimisticConcurrency(
        { datasourceType: "standard", optimisticConcurrency: false },
        true,
      ),
      false,
    );
    assert.equal(
      entityUsesOptimisticConcurrency(
        { datasourceType: "standard", optimisticConcurrency: true },
        false,
      ),
      true,
    );
    assert.equal(
      entityUsesOptimisticConcurrency({ datasourceType: "standard" }, true),
      true,
    );
    assert.equal(
      entityUsesOptimisticConcurrency({ datasourceType: "standard" }, false),
      false,
    );
  });
});

describe("parseDatasourceTypes target and optimisticConcurrency", () => {
  it("parses target and use_optimistic_concurrency onto DatasourceType", () => {
    const types = parseDatasourceTypes({
      idType: "integer",
      yaml: `types:
  - sink:
      target: None
      fields:
        - label:
            type: string
  - app:
      use_optimistic_concurrency: false
      fields:
        - name:
            type: string
`,
    });
    assert.equal(types[0]?.target, "None");
    assert.equal(types[1]?.optimisticConcurrency, false);
  });
});
