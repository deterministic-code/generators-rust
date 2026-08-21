import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  DeterministicParser,
  ROUTES_YAML,
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
import { Emit } from "./emit.ts";

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

class Generator extends Emit {
  private readonly datasources: ExpandedDatasourceType[];

  constructor(
    raw: Record<string, string>,
    datasources: ExpandedDatasourceType[],
  ) {
    super(raw);
    this.datasources = datasources;
  }

  from(deterministic: IDeterministic): GenerateEntry[] {
    return deterministic.routes.candidates.map((c) => this.test(c));
  }

  private test(candidate: RouteCandidate): GenerateEntry {
    const table = this.datasources.find((d) => d.name === candidate.name);
    const path = this.imports.routeTest(candidate.name);
    const mountPath = `/api/${this.imports.apiPath(candidate.name)}`;
    const occ = this.settings.usesOptimisticConcurrency(candidate);
    const fileBase = this.imports.routeModule(candidate.name);
    const shared = {
      fileBase,
      serviceImport: this.imports.serviceUse(
        candidate.name,
        this.casing.serviceClassName(candidate.name),
      ),
      className: this.casing.serviceClassName(candidate.name),
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
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(ROUTES_YAML);
  const deterministic = await DeterministicParser(ctx.reader).parse(
    ctx.settings,
  );
  return new Generator(
    ctx.settings,
    deterministic.expandedDatasourceTypes,
  ).from(deterministic);
};
