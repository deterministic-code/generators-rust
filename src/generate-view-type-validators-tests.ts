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
  type DatasourceType,
} from "./common/parse-datasource-types.ts";
import {
  loadViewTypes,
  type ShapedView,
  type ViewField,
  type ViewType,
} from "./common/parse-view-types.ts";
import { settingsStr } from "./common/settings.ts";
import { convertSpecType } from "./common/type-converter.ts";
import { typeTestTmpl } from "./view-type-validators-tests/resources.ts";

type EmitOptions = {
  ds: DatasourceSettings;
  naming: ArtifactNaming;
  schemaVersion: string;
  tables: Map<string, DatasourceType>;
  views: Map<string, ViewType>;
};

type FieldTok = {
  ident: string;
  sampleExpr: string;
  nullable: boolean;
};

type CaseTok = {
  ident: string;
  fixture: string;
  assertion: string;
};

const emitBase = (settings: SettingsDict) => ({
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

const wrapValue = (
  expr: string,
  field: { isArray: boolean; isNullable: boolean },
): string => {
  const inner = field.isArray ? `vec![${expr}]` : expr;
  return field.isNullable ? `Some(${inner})` : inner;
};

const qual = (
  entity: string,
  kind: "datasource" | "view",
  naming: ArtifactNaming,
): string => {
  const cls = naming.className(entity);
  if (naming.byFeature) {
    const stem = naming.filePath(entity).replace(/\.rs$/, "");
    return `crate::${stem.split("/").join("::")}::${cls}`;
  }
  const ns =
    kind === "datasource"
      ? "crate::types::generated::datasource"
      : "crate::types::generated::views";
  return `${ns}::${cls}`;
};

const renderDs = (name: string, opts: EmitOptions): string => {
  const table = opts.tables.get(name);
  const cls = qual(name, "datasource", opts.naming);
  if (table === undefined) return `${cls} {}`;
  const body = tableFields(table.fields, opts.ds)
    .map((f) => {
      const { sample } = samplesForNative(nativeFieldType(opts.ds, f), f.type);
      const val = f.isNullable ? `Some(${sample})` : sample;
      return `${opts.naming.fieldName(f.name)}: ${val}`;
    })
    .join(", ");
  return `${cls} { ${body} }`;
};

const parentToks = (view: ShapedView, opts: EmitOptions): FieldTok[] => {
  if (view.inherits === null) return [];
  const table = opts.tables.get(view.inherits);
  if (table === undefined) return [];
  const omit = new Set([
    ...view.omit,
    ...view.enrichments.map((e) => e.fkColumn),
  ]);
  return tableFields(table.fields, opts.ds)
    .filter((f) => !omit.has(f.name))
    .map((f) => {
      const pair = samplesForNative(nativeFieldType(opts.ds, f), f.type);
      return {
        ident: opts.naming.fieldName(f.name),
        sampleExpr: f.isNullable ? `Some(${pair.sample})` : pair.sample,
        nullable: f.isNullable,
      };
    });
};

const viewFieldTok = (
  field: ViewField,
  opts: EmitOptions,
  visited: Set<string>,
): FieldTok => {
  let sample: string;
  if (field.kind === "primitive") {
    sample = samplesForNative(
      convertSpecType(field.base, opts.ds.datetimeRepr),
      field.base,
    ).sample;
  } else if (field.kind === "datasource") {
    sample = renderDs(field.base, opts);
  } else {
    sample = viewFixture(field.base, opts, visited);
  }
  return {
    ident: opts.naming.fieldName(field.name),
    sampleExpr: wrapValue(sample, field),
    nullable: field.isNullable,
  };
};

const shapedToks = (
  view: ShapedView,
  opts: EmitOptions,
  visited: Set<string>,
): FieldTok[] => {
  const declared = view.fields.map((f) => viewFieldTok(f, opts, visited));
  const inline = view.enrichments.length > 0 || view.omit.length > 0;
  const alias = view.fields.length === 0 && !inline;
  if (view.inherits === null) return declared;
  if (alias || inline) return [...parentToks(view, opts), ...declared];
  const base = renderDs(view.inherits, opts);
  return [{ ident: "base", sampleExpr: base, nullable: false }, ...declared];
};

const viewFixture = (
  name: string,
  opts: EmitOptions,
  visited: Set<string>,
): string => {
  if (visited.has(name)) return "{}";
  const view = opts.views.get(name);
  if (view === undefined) return "{}";
  const next = new Set(visited).add(name);
  if (view.kind === "union") {
    const member = view.members[0];
    if (member === undefined) return `${qual(name, "view", opts.naming)} {}`;
    return `${qual(name, "view", opts.naming)}::${opts.naming.className(member)}(${viewFixture(member, opts, next)})`;
  }
  const cls = opts.naming.className(view.name);
  const fields = shapedToks(view, opts, next);
  return `${cls} { ${fields.map((f) => `${f.ident}: ${f.sampleExpr}`).join(", ")} }`;
};

const testPath = (entity: string, naming: ArtifactNaming): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const shapedCases = (view: ShapedView, opts: EmitOptions): CaseTok[] => {
  const fields = shapedToks(view, opts, new Set([view.name]));
  const cls = opts.naming.className(view.name);
  const valid = `${cls} { ${fields.map((f) => `${f.ident}: ${f.sampleExpr}`).join(", ")} }`;
  const cases: CaseTok[] = [
    { ident: "parses_a_valid_payload", fixture: valid, assertion: "is_ok()" },
  ];
  if (fields.some((f) => f.nullable)) {
    cases.push({
      ident: "accepts_none_for_nullable_fields",
      fixture: `${cls} { ${fields.map((f) => `${f.ident}: ${f.nullable ? "None" : f.sampleExpr}`).join(", ")} }`,
      assertion: "is_ok()",
    });
  }
  return cases;
};

const unionCases = (
  view: Extract<ViewType, { kind: "union" }>,
  opts: EmitOptions,
): CaseTok[] =>
  view.members.map((name) => ({
    ident: `accepts_${opts.naming.fieldName(name)}_member`,
    fixture: `${opts.naming.className(view.name)}::${opts.naming.className(name)}(${viewFixture(name, opts, new Set([view.name]))})`,
    assertion: "is_ok()",
  }));

const renderTests = (view: ViewType, opts: EmitOptions): GenerateEntry =>
  content(
    testPath(view.name, opts.naming),
    fill(typeTestTmpl, {
      schemaVersion: opts.schemaVersion,
      typeUse: qual(view.name, "view", opts.naming),
      fnName: `validate_${snakeCase(view.name)}`,
      cases:
        view.kind === "union" ? unionCases(view, opts) : shapedCases(view, opts),
    }),
  );

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const base = emitBase(ctx.settings);
  const views = await loadViewTypes(ctx.reader);
  const tables = (await ctx.reader.exists(DATASOURCE_TYPES_YAML))
    ? parseDatasourceTypes({
        yaml: await ctx.reader.read(DATASOURCE_TYPES_YAML),
        idType: base.ds.idType,
      })
    : [];
  const opts: EmitOptions = {
    ...base,
    tables: new Map(tables.map((t) => [t.name, t])),
    views: new Map(views.map((v) => [v.name, v])),
  };
  return views.map((view) => renderTests(view, opts));
};
