import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { datasourcePaths, type ArtifactPaths } from "./common/paths.ts";
import {
  tableFields,
  SpecificationParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceType,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTestTmpl } from "./resources/datasource-types-tests.ts";

type Datasource = {
  idType: string;
  withUuidColumn: boolean;
  useOptimisticConcurrency: boolean;
};

const datasource = (settings: Record<string, string>): Datasource => {
  const idType = settings["datasource.id_type"] ?? "integer";
  return {
    idType,
    withUuidColumn: idType !== "uuid",
    useOptimisticConcurrency:
      settings["datasource.use_optimistic_concurrency"] === "true",
  };
};

type EmitOptions = {
  ds: Datasource;
  naming: ArtifactPaths;
  schemaVersion: string;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  ds: datasource(settings),
  naming: datasourcePaths(settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
});

const rustString = (value: string): string => `String::from(${JSON.stringify(value)})`;

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
      if (fieldType === "date") {
        return {
          sample: rustString("2024-01-01"),
          next: rustString("2024-01-02"),
        };
      }
      if (fieldType === "email") {
        return {
          sample: rustString("sample@example.com"),
          next: rustString("next@example.com"),
        };
      }
      return { sample: rustString("sample"), next: rustString("sample-next") };
    default:
      throw new Error(`Unknown rust native type: ${native}`);
  }
};

const wrapOption = (expr: string, nullable: boolean): string =>
  nullable ? `Some(${expr})` : expr;

const fieldTokens = (
  field: { name: string; type: string; isNullable: boolean },
  opts: EmitOptions,
) => {
  const ident = opts.naming.fieldName(field.name);
  const native = convertSpecType(field.type);
  const { sample, next } = samplesForNative(native, field.type);
  return {
    ident,
    sampleExpr: wrapOption(sample, field.isNullable),
    nextExpr: wrapOption(next, field.isNullable),
    nullable: field.isNullable,
  };
};

const testPath = (entity: string, naming: ArtifactPaths): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const renderTests = (
  table: DatasourceType,
  opts: EmitOptions,
): GenerateEntry => {
  const fields = tableFields(table.fields, opts.ds.idType).map((f) =>
    fieldTokens(f, opts),
  );
  return content(
    testPath(table.name, opts.naming),
    fill(typeTestTmpl, {
      schemaVersion: opts.schemaVersion,
      structName: opts.naming.className(table.name),
      fileBase: opts.naming.fileBase(table.name),
      fields,
    }),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const opts = emitOptions(ctx.settings);
  const types = new SpecificationParser().parseDatasourceTypes({
    yaml: await ctx.reader.read(DATASOURCE_TYPES_YAML),
    idType: opts.ds.idType,
  });
  return types.map((table) => renderTests(table, opts));
};
