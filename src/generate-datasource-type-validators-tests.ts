import { snakeCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { datasourcePaths, type ArtifactPaths } from "./common/paths.ts";
import {
  rustString,
  samplesForNative,
  wrapOption,
} from "./common/test-samples.ts";
import {
  DeterministicParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceField,
  type DatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTestTmpl } from "./resources/datasource-type-validators-tests.ts";

type EmitOptions = {
  naming: ArtifactPaths;
  schemaVersion: string;
};

type FieldTok = {
  name: string;
  ident: string;
  sampleExpr: string;
  isNullable: boolean;
  type: string;
  native: string;
};

type CaseTok = {
  ident: string;
  fixture: string;
  assertion: string;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  naming: datasourcePaths(settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
});

const fieldTok = (
  field: DatasourceField | { name: string; type: string; isNullable: boolean },
  opts: EmitOptions,
): FieldTok => {
  const native = convertSpecType(field.type);
  const { sample } = samplesForNative(native, field.type);
  return {
    name: field.name,
    ident: opts.naming.fieldName(field.name),
    sampleExpr: wrapOption(sample, field.isNullable),
    isNullable: field.isNullable,
    type: field.type,
    native,
  };
};

const structLiteral = (
  cls: string,
  fields: Array<{ ident: string; expr: string }>,
): string =>
  `${cls} { ${fields.map((f) => `${f.ident}: ${f.expr}`).join(", ")} }`;

const typeUse = (entity: string, naming: ArtifactPaths): string => {
  const cls = naming.className(entity);
  if (naming.byFeature) {
    const stem = naming.filePath(entity).replace(/\.rs$/, "");
    return `crate::${stem.split("/").join("::")}::${cls}`;
  }
  return `crate::types::generated::datasource::${cls}`;
};

const testPath = (entity: string, naming: ArtifactPaths): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const casesFor = (cls: string, fields: FieldTok[]): CaseTok[] => {
  const valid = structLiteral(
    cls,
    fields.map((f) => ({ ident: f.ident, expr: f.sampleExpr })),
  );
  const cases: CaseTok[] = [
    { ident: "parses_a_valid_payload", fixture: valid, assertion: "is_ok()" },
  ];
  if (fields.some((f) => f.isNullable)) {
    cases.push({
      ident: "accepts_none_for_nullable_fields",
      fixture: structLiteral(
        cls,
        fields.map((f) => ({
          ident: f.ident,
          expr: f.isNullable ? "None" : f.sampleExpr,
        })),
      ),
      assertion: "is_ok()",
    });
  }
  for (const field of fields) {
    if (field.type === "uuid" && field.native === "String") {
      cases.push({
        ident: `rejects_when_invalid_uuid_on_${field.ident}`,
        fixture: structLiteral(
          cls,
          fields.map((f) => ({
            ident: f.ident,
            expr:
              f.ident === field.ident
                ? wrapOption(rustString("not-a-uuid"), f.isNullable)
                : f.sampleExpr,
          })),
        ),
        assertion: "is_err()",
      });
    }
  }
  return cases;
};

const renderTests = (
  table: DatasourceType,
  opts: EmitOptions,
): GenerateEntry => {
  const fields = table.fields.map((f) => fieldTok(f, opts));
  const cls = opts.naming.className(table.name);
  return content(
    testPath(table.name, opts.naming),
    fill(typeTestTmpl, {
      schemaVersion: opts.schemaVersion,
      typeUse: typeUse(table.name, opts.naming),
      fnName: `validate_datasource_${snakeCase(table.name)}`,
      cases: casesFor(cls, fields),
    }),
  );
};

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const opts = emitOptions(settings);
  return deterministic.expandedDatasourceTypes.map((table) =>
    renderTests(table, opts),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(DATASOURCE_TYPES_YAML);
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
    ctx.settings,
  );
};
