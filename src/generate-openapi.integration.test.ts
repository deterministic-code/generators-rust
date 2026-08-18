import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "./common/deterministic-reader.ts";
import type { GenerateEntry } from "./common/generate-entry.ts";
import { generate } from "./generate-openapi.ts";

const textOf = (entries: GenerateEntry[], path: string): string => {
  const hit = entries.find((e) => e.kind === "content" && e.filename === path);
  assert.ok(hit, `missing ${path}`);
  assert.equal(hit.kind, "content");
  return hit.contents;
};

describe("generate-openapi", () => {
  it("embeds a spec for each route candidate", async () => {
    const entries = await generate({
      reader: memoryReader({
        "datasource_types.yaml": `types:
  - user:
      fields:
        - email:
            type: string
`,
        "view_types.yaml": `includes:
  - datasource_types:
      include: "*"
types: []
`,
        "routes.yaml": `includes:
  - view_type_routes:
      filter: 'type is view_type || type is datasource_type'
routes: []
`,
      }),
      settings: { application_name: "Demo", "codegen.schema_version": "2.0" },
    });
    const router = textOf(entries, "openapi.rs");
    assert.match(router, /OPENAPI_JSON/);
    assert.match(router, /\/api\/users/);
    assert.match(router, /Demo/);

    const test = textOf(entries, "openapi_conformance.rs");
    assert.match(test, /use consumer::routes::openapi/);
    assert.match(test, /"\/api\/users"/);
  });

  it("rejects a missing routes.yaml", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({}),
          settings: {},
        }),
      /routes\.yaml/,
    );
  });
});
