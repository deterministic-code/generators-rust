import { pascalCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  createImportGenerator,
  type RustImportGenerator,
} from "./import-generator.ts";
import {
  DeterministicParser,
  ROUTES_YAML,
  entityUsesOptimisticConcurrency,
  type ExpandedDatasourceType,
  type RouteByField,
  type RouteCandidate,
  type IDeterministic,
} from "./specification-parser.ts";
import {
  byFieldGetListTmpl,
  byFieldGetUniqueTmpl,
  crudTmpl,
  readonlyTmpl,
} from "./resources/routes-tests.ts";

type EmitOptions = {
  imports: RustImportGenerator;
  useOcc: boolean;
  datasources: ExpandedDatasourceType[];
};

const serviceClassName = (entity: string): string =>
  pascalCase(`${entity}_service`);

const missingIdExpr = (idType: string): string =>
  idType === "uuid"
    ? "00000000-0000-0000-0000-000000000000"
    : "99999";

const getByFields = (byFields: RouteByField[]): RouteByField[] =>
  byFields.filter((entry) => {
    const methods = Array.isArray(entry.methods) ? entry.methods : ["GET"];
    return methods.includes("GET");
  });

const byFieldsBlock = (
  entitySnake: string,
  mountPath: string,
  byFields: RouteByField[],
): string =>
  getByFields(byFields)
    .map((entry) =>
      fill(entry.byFieldUnique ? byFieldGetUniqueTmpl : byFieldGetListTmpl, {
        entitySnake,
        mountPath,
        byField: entry.byField,
        kebab: entry.byField.replace(/_/g, "-"),
      }),
    )
    .join("");

const renderTest = (
  candidate: RouteCandidate,
  opts: EmitOptions,
): GenerateEntry => {
  const table = opts.datasources.find((d) => d.name === candidate.name);
  const path = opts.imports.routeTest(candidate.name);
  const mountPath = `/api/${opts.imports.apiPath(candidate.name)}`;
  const occ = entityUsesOptimisticConcurrency(candidate, opts.useOcc);
  const fileBase = opts.imports.routeModule(candidate.name);
  const shared = {
    fileBase,
    serviceImport: opts.imports.serviceUse(
      candidate.name,
      serviceClassName(candidate.name),
    ),
    className: serviceClassName(candidate.name),
    entitySnake: candidate.name,
    mountPath,
    missingId: missingIdExpr(
      table?.fields.find((f) => f.name === (table.primaryKeyColumn ?? "id"))
        ?.type ?? "integer",
    ),
    occ,
    byFieldsBlock: byFieldsBlock(candidate.name, mountPath, candidate.byFields),
  };
  if (candidate.datasourceType === "readonly-lookup") {
    return content(path, fill(readonlyTmpl, shared));
  }
  return content(path, fill(crudTmpl, shared));
};

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const parsed = deterministic.routes;
  const opts: EmitOptions = {
    imports: createImportGenerator(".", settings),
    useOcc: settings["datasource.use_optimistic_concurrency"] !== "false",
    datasources: deterministic.expandedDatasourceTypes,
  };
  return parsed.candidates.map((c) => renderTest(c, opts));
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(ROUTES_YAML);
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
    ctx.settings,
  );
};
