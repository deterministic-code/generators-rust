import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
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
import { Emit } from "./emit.ts";

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

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    const tables = deterministic.expandedDatasourceTypes.filter(
      (t) => t.datasourceType !== "many-to-many",
    );
    const setups = tables
      .map((t) =>
        fill(setupTmpl, {
          entity: t.name,
          segment: this.imports.apiPath(t.name),
          setupFn: this.casing.fnIdent(`setup_${t.name}_router`),
          listRouteName: this.casing.fnIdent(`list_${t.name}`),
          getByIdRouteName: this.casing.fnIdent(`get_${t.name}_by_id`),
          createRouteName: this.casing.fnIdent(`create_${t.name}`),
          deleteByIdRouteName: this.casing.fnIdent(`delete_${t.name}_by_id`),
        }).trimEnd(),
      )
      .join("\n\n");
    const tests = tables
      .map((t) => {
        const tokens = {
          entity: t.name,
          pascal: this.casing.convertTypes(t.name),
          segment: this.imports.apiPath(t.name),
          setupFn: this.casing.fnIdent(`setup_${t.name}_router`),
          listTest: this.casing.fnIdent(`${t.name}_list_returns_200`),
          postTest: this.casing.fnIdent(`${t.name}_post_accepts_sample_payload`),
          getMissingTest: this.casing.fnIdent(
            `${t.name}_get_missing_returns_404`,
          ),
          deleteMissingTest: this.casing.fnIdent(
            `${t.name}_delete_missing_returns_non_5xx`,
          ),
          missing:
            (
              t.fields.find((f) => f.isPrimaryKey === true)?.type ??
              "integer"
            ) === "uuid"
              ? "00000000-0000-0000-0000-000000000000"
              : "99999",
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
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  if (!(await ctx.reader.exists(DATASOURCE_TYPES_YAML))) return [];
  await ctx.reader.read(DATASOURCE_TYPES_YAML);
  return new Generator(ctx.settings).from(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  );
};
