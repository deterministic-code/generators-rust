import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { datasourcePaths, routePaths } from "./common/paths.ts";
import {
  DeterministicParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import {
  crudTestsTmpl,
  fileTmpl,
  readonlyTestsTmpl,
  setupTmpl,
} from "./resources/routes-e2e.ts";

const jsonSample = (type: string): string => {
  switch (type) {
    case "boolean":
      return "true";
    case "number":
    case "integer":
    case "smallinteger":
    case "biginteger":
    case "float":
    case "reference":
      return "1";
    case "datetime":
      return `"2020-01-01T00:00:00.000Z"`;
    case "uuid":
      return `"00000000-0000-0000-0000-000000000001"`;
    default:
      return `"x"`;
  }
};

const samplePayload = (table: DatasourceType): string => {
  const parts = table.fields
    .filter(
      (f) =>
        !f.isNullable &&
        f.hasDefault !== true &&
        f.references === undefined,
    )
    .map((f) => `"${f.name}":${jsonSample(f.type)}`);
  return `{${parts.join(",")}}`;
};

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const idType = settings["datasource.id_type"] ?? "integer";
  const naming = routePaths(settings);
  const names = datasourcePaths(settings);
  const tables = deterministic.expandedDatasourceTypes.filter(
    (t) => t.datasourceType !== "many-to-many",
  );
  const missing =
    idType === "uuid" ? "00000000-0000-0000-0000-000000000000" : "99999";
  const setups = tables
    .map((t) =>
      fill(setupTmpl, {
        entity: t.name,
        segment: naming.apiPath(t.name),
      }).trimEnd(),
    )
    .join("\n\n");
  const tests = tables
    .map((t) => {
      const tokens = {
        entity: t.name,
        pascal: names.className(t.name),
        segment: naming.apiPath(t.name),
        missing,
        payload: samplePayload(t),
      };
      const tmpl =
        t.datasourceType === "readonly-lookup"
          ? readonlyTestsTmpl
          : crudTestsTmpl;
      return fill(tmpl, tokens).trimEnd();
    })
    .join("\n\n");
  return [
    content(
      "app_routes_e2e.rs",
      fill(fileTmpl, { setups, tests }),
    ),
  ];
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  if (!(await ctx.reader.exists(DATASOURCE_TYPES_YAML))) return [];
  await ctx.reader.read(DATASOURCE_TYPES_YAML);
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
    ctx.settings,
  );
};
