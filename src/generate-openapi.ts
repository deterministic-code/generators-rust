import { fill } from "./common/fill.ts";
import type { GenerateContext } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import { datasourceSettings } from "./common/datasource-settings.ts";
import { rustRouteNaming } from "./common/naming.ts";
import { loadRoutes } from "./common/parse-routes.ts";
import { settingsStr } from "./common/settings.ts";
import { conformanceTmpl, routerTmpl } from "./resources/openapi.ts";

const jsonObjectContent = {
  "application/json": { schema: { type: "object" } },
};
const okObjectResponse = { description: "OK", content: jsonObjectContent };
const notFoundResponse = { description: "Not Found" };
const idParameter = {
  name: "id",
  in: "path",
  required: true,
  schema: { type: "integer", format: "int64" },
};
const objectRequestBody = { required: true, content: jsonObjectContent };

type OpenApiDoc = {
  openapi: string;
  info: { title: string; version: string };
  tags: { name: string }[];
  paths: Record<string, unknown>;
  components: { schemas: Record<string, unknown> };
};

const escapeRustStringLiteral = (s: string): string =>
  s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n");

const collectionOps = (tag: string) => ({
  get: {
    tags: [tag],
    summary: `List ${tag}`,
    responses: {
      200: {
        description: "OK",
        content: {
          "application/json": {
            schema: { type: "array", items: { type: "object" } },
          },
        },
      },
    },
  },
  post: {
    tags: [tag],
    summary: `Create ${tag}`,
    requestBody: objectRequestBody,
    responses: {
      201: { description: "Created", content: jsonObjectContent },
    },
  },
});

const memberOps = (tag: string) => ({
  get: {
    tags: [tag],
    summary: `Get one ${tag}`,
    parameters: [idParameter],
    responses: { 200: okObjectResponse, 404: notFoundResponse },
  },
  put: {
    tags: [tag],
    summary: `Update ${tag}`,
    parameters: [idParameter],
    requestBody: objectRequestBody,
    responses: { 200: okObjectResponse, 404: notFoundResponse },
  },
  patch: {
    tags: [tag],
    summary: `Partially update ${tag}`,
    parameters: [idParameter],
    requestBody: objectRequestBody,
    responses: { 200: okObjectResponse, 404: notFoundResponse },
  },
  delete: {
    tags: [tag],
    summary: `Delete ${tag}`,
    parameters: [idParameter],
    responses: { 200: okObjectResponse, 404: notFoundResponse },
  },
});

const buildSpec = (
  title: string,
  version: string,
  entities: string[],
  apiPath: (entity: string) => string,
): OpenApiDoc => {
  const sorted = [...entities].sort();
  const paths: Record<string, unknown> = {};
  for (const entity of sorted) {
    const base = `/api/${apiPath(entity)}`;
    const tag = apiPath(entity);
    paths[base] = collectionOps(tag);
    paths[`${base}/{id}`] = memberOps(tag);
  }
  return {
    openapi: "3.0.3",
    info: { title, version },
    tags: sorted.map((e) => ({ name: apiPath(e) })),
    paths,
    components: { schemas: {} },
  };
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const naming = rustRouteNaming(ctx.settings);
  const title = settingsStr(ctx.settings, "application_name") ?? "API";
  const version =
    settingsStr(ctx.settings, "codegen.schema_version") ?? "0.0.0";
  const crateName =
    settingsStr(ctx.settings, "languages.rust.crate_name") ?? "consumer";
  const parsed = await loadRoutes(ctx.reader, {
    idType: datasourceSettings(ctx.settings).idType,
  });
  const entities = parsed.candidates.map((c) => c.name);
  const spec = buildSpec(title, version, entities, naming.apiPath);
  const expectedPaths = Object.keys(spec.paths)
    .sort()
    .map((p) => `        ${JSON.stringify(p)},`)
    .join("\n");
  return [
    content(
      "openapi.rs",
      fill(routerTmpl, {
        specJson: escapeRustStringLiteral(JSON.stringify(spec)),
      }),
    ),
    content(
      "openapi_conformance.rs",
      fill(conformanceTmpl, {
        crateModule: crateName.replace(/-/g, "_"),
        expectedPaths,
      }),
    ),
  ];
};
