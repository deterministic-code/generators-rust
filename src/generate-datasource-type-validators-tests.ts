import { pascalCase, snakeCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  createImportGenerator,
  type RustImportGenerator,
} from "./import-generator.ts";
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
  imports: RustImportGenerator;
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

const className = (entity: string): string => pascalCase(entity);
const fieldName = (field: string): string => field;

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  imports: createImportGenerator(".", settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
});

const fieldTok = (
  field: DatasourceField | { name: string; type: string; isNullable: boolean },
): FieldTok => {
  const native = convertSpecType(field.type);
  const { sample } = samplesForNative(native, field.type);
  return {
    name: field.name,
    ident: fieldName(field.name),
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

const typeUse = (entity: string, imports: RustImportGenerator): string =>
  imports.datasourceQual(entity);

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
  const fields = table.fields.map((f) => fieldTok(f));
  const cls = className(table.name);
  const src = opts.imports.datasource(table.name);
  return content(
    opts.imports.test(src, table.name),
    fill(typeTestTmpl, {
      schemaVersion: opts.schemaVersion,
      typeUse: typeUse(table.name, opts.imports),
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
