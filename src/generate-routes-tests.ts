import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  DeterministicParser,
  ROUTES_YAML,
  type DatasourceType,
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
  fnIdent: (stem: string) => string,
): string =>
  getByFields(byFields)
    .map((entry) =>
      fill(entry.byFieldUnique ? byFieldGetUniqueTmpl : byFieldGetListTmpl, {
        entitySnake,
        mountPath,
        byField: entry.byField,
        kebab: entry.byField.replace(/_/g, "-"),
        byFieldMissingTest: fnIdent(
          `get_${entitySnake}_by_${entry.byField}_missing_returns_404`,
        ),
        byFieldListTest: fnIdent(
          `get_${entitySnake}_by_${entry.byField}_returns_items`,
        ),
      }),
    )
    .join("");

class Generator extends Emit {
  private readonly datasources: DatasourceType[];

  constructor(
    raw: Record<string, string>,
    datasources: DatasourceType[],
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
        table?.fields.find((f) => f.isPrimaryKey === true)?.type ??
          "integer",
      ),
      occ,
      getListTest: this.casing.fnIdent(
        `get_${candidate.name}_list_returns_200`,
      ),
      getMissingTest: this.casing.fnIdent(
        `get_${candidate.name}_missing_returns_404`,
      ),
      postTest: this.casing.fnIdent(`post_${candidate.name}_returns_201`),
      putMissingTest: this.casing.fnIdent(
        `put_${candidate.name}_missing_is_not_5xx`,
      ),
      patchMissingTest: this.casing.fnIdent(
        `patch_${candidate.name}_missing_is_not_5xx`,
      ),
      deleteMissingTest: this.casing.fnIdent(
        `delete_${candidate.name}_missing_is_not_5xx`,
      ),
      byFieldsBlock: byFieldsBlock(
        candidate.name,
        mountPath,
        candidate.byFields,
        (stem) => this.casing.fnIdent(stem),
      ),
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
