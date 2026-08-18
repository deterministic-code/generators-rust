import { snakeCase } from "change-case";
import {
  datasourceSettings,
  nativeFieldType,
  tableFields,
  type DatasourceSettings,
} from "./common/datasource-settings.ts";
import { fill } from "./common/fill.ts";
import type { GenerateContext, SettingsDict } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import { rustNaming, type ArtifactNaming } from "./common/naming.ts";
import {
  DATASOURCE_TYPES_YAML,
  parseDatasourceTypes,
  type DatasourceField,
  type DatasourceType,
} from "./common/parse-datasource-types.ts";
import { settingsStr } from "./common/settings.ts";
import { typeTestTmpl } from "./resources/datasource-type-validators-tests.ts";

type EmitOptions = {
  ds: DatasourceSettings;
  naming: ArtifactNaming;
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

const emitOptions = (settings: SettingsDict): EmitOptions => ({
  ds: datasourceSettings(settings),
  naming: rustNaming(settings),
  schemaVersion: settingsStr(settings, "codegen.schema_version") ?? "1.0",
});

const rustString = (value: string): string =>
  `String::from(${JSON.stringify(value)})`;

const rustDatetime = (iso: string): string =>
  `chrono::DateTime::parse_from_rfc3339(${JSON.stringify(iso)}).unwrap().with_timezone(&chrono::Utc)`;

const samplesForNative = (
  native: string,
  fieldType: string,
): { sample: string; next: string } => {
  switch (native) {
    case "i64":
      return { sample: "1i64", next: "2i64" };
    case "i32":
      return { sample: "1i32", next: "2i32" };
    case "i16":
      return { sample: "1i16", next: "2i16" };
    case "u64":
      return { sample: "1u64", next: "2u64" };
    case "u32":
      return { sample: "1u32", next: "2u32" };
    case "u16":
      return { sample: "1u16", next: "2u16" };
    case "f64":
      return { sample: "1.0f64", next: "2.0f64" };
    case "bool":
      return { sample: "false", next: "true" };
    case "uuid::Uuid":
      return {
        sample: "uuid::Uuid::nil()",
        next: "uuid::Uuid::from_u128(1)",
      };
    case "Vec<u8>":
      return { sample: "Vec::<u8>::new()", next: "vec![1u8]" };
    case "chrono::DateTime<chrono::Utc>":
      return {
        sample: rustDatetime("2024-01-01T00:00:00.000Z"),
        next: rustDatetime("2024-01-02T00:00:00.000Z"),
      };
    case "String":
      if (fieldType === "decimal") {
        return { sample: rustString("0"), next: rustString("1") };
      }
      if (fieldType === "uuid") {
        return {
          sample: rustString("00000000-0000-0000-0000-000000000000"),
          next: rustString("00000000-0000-0000-0000-000000000001"),
        };
      }
      if (fieldType === "datetime") {
        return {
          sample: rustString("2024-01-01T00:00:00.000Z"),
          next: rustString("2024-01-02T00:00:00.000Z"),
        };
      }
      return { sample: rustString("sample"), next: rustString("sample-next") };
    default:
      throw new Error(`Unknown rust native type: ${native}`);
  }
};

const wrapOption = (expr: string, nullable: boolean): string =>
  nullable ? `Some(${expr})` : expr;

const fieldTok = (
  field: DatasourceField | { name: string; type: string; isNullable: boolean },
  opts: EmitOptions,
): FieldTok => {
  const native = nativeFieldType(opts.ds, field);
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

const typeUse = (entity: string, naming: ArtifactNaming): string => {
  const cls = naming.className(entity);
  if (naming.byFeature) {
    const stem = naming.filePath(entity).replace(/\.rs$/, "");
    return `crate::${stem.split("/").join("::")}::${cls}`;
  }
  return `crate::types::generated::datasource::${cls}`;
};

const testPath = (entity: string, naming: ArtifactNaming): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const casesFor = (cls: string, fields: FieldTok[], declared: FieldTok[]): CaseTok[] => {
  const valid = structLiteral(
    cls,
    fields.map((f) => ({ ident: f.ident, expr: f.sampleExpr })),
  );
  const cases: CaseTok[] = [
    { ident: "parses_a_valid_payload", fixture: valid, assertion: "is_ok()" },
  ];
  if (declared.some((f) => f.isNullable)) {
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
  for (const field of declared) {
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
  const fields = tableFields(table.fields, opts.ds).map((f) =>
    fieldTok(f, opts),
  );
  const declared = table.fields.map((f) => fieldTok(f, opts));
  const cls = opts.naming.className(table.name);
  return content(
    testPath(table.name, opts.naming),
    fill(typeTestTmpl, {
      schemaVersion: opts.schemaVersion,
      typeUse: typeUse(table.name, opts.naming),
      fnName: `validate_datasource_${snakeCase(table.name)}`,
      cases: casesFor(cls, fields, declared),
    }),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const opts = emitOptions(ctx.settings);
  const types = parseDatasourceTypes({
    yaml: await ctx.reader.read(DATASOURCE_TYPES_YAML),
    idType: opts.ds.idType,
  });
  return types.map((table) => renderTests(table, opts));
};
