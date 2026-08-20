import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { routePaths, type RoutePaths } from "./common/paths.ts";
import {
  SpecificationParser,
  primaryKeyFor,
  entityUsesOptimisticConcurrency,
  type DatasourceType,
  type RouteByField,
  type RouteCandidate,
} from "./specification-parser.ts";
import {
  byFieldGetListTmpl,
  byFieldGetUniqueTmpl,
  crudTmpl,
  readonlyTmpl,
} from "./resources/routes-tests.ts";

type EmitOptions = {
  naming: RoutePaths;
  idType: string;
  useOcc: boolean;
  datasources: DatasourceType[];
};

const testPath = (entity: string, naming: RoutePaths): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

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
  const pk = primaryKeyFor(candidate.name, opts.datasources, opts.idType);
  const path = testPath(candidate.name, opts.naming);
  const mountPath = `/api/${opts.naming.apiPath(candidate.name)}`;
  const occ = entityUsesOptimisticConcurrency(candidate, opts.useOcc);
  const shared = {
    fileBase: opts.naming.fileBase(candidate.name),
    serviceImport: opts.naming.serviceUseLine(
      candidate.name,
      opts.naming.serviceClassName(candidate.name),
    ),
    className: opts.naming.serviceClassName(candidate.name),
    entitySnake: candidate.name,
    mountPath,
    missingId: missingIdExpr(pk.idType),
    occ,
    byFieldsBlock: byFieldsBlock(candidate.name, mountPath, candidate.byFields),
  };
  if (candidate.datasourceType === "readonly-lookup") {
    return content(path, fill(readonlyTmpl, shared));
  }
  return content(path, fill(crudTmpl, shared));
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const idType = ctx.settings["datasource.id_type"] ?? "integer";
  const parsed = await new SpecificationParser(ctx.reader).loadRoutes({
    idType,
  });
  const opts: EmitOptions = {
    naming: routePaths(ctx.settings),
    idType,
    useOcc: ctx.settings["datasource.use_optimistic_concurrency"] === "true",
    datasources: parsed.datasources,
  };
  return parsed.candidates.map((c) => renderTest(c, opts));
};
