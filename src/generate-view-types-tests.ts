import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { viewPaths, type ArtifactPaths } from "./common/paths.ts";
import {
  tableFields,
  SpecificationParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceType,
  type ShapedView,
  type ViewField,
  type ViewType,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTestTmpl } from "./resources/view-types-tests.ts";

type EmitOptions = {
  idType: string;
  naming: ArtifactPaths;
  schemaVersion: string;
  tables: Map<string, DatasourceType>;
  views: Map<string, ViewType>;
};

type FieldTok = {
  ident: string;
  sampleExpr: string;
  nextExpr: string;
  nullable: boolean;
};

const emitBase = (settings: Record<string, string>) => ({
  idType: settings["datasource.id_type"] ?? "integer",
  naming: viewPaths(settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
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
  naming: ArtifactPaths,
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
  const body = tableFields(table.fields, opts.idType)
    .map((f) => {
      const { sample } = samplesForNative(convertSpecType(f.type), f.type);
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
  return tableFields(table.fields, opts.idType)
    .filter((f) => !omit.has(f.name))
    .map((f) => {
      const pair = samplesForNative(convertSpecType(f.type), f.type);
      return {
        ident: opts.naming.fieldName(f.name),
        sampleExpr: f.isNullable ? `Some(${pair.sample})` : pair.sample,
        nextExpr: f.isNullable ? `Some(${pair.next})` : pair.next,
        nullable: f.isNullable,
      };
    });
};

const viewFieldTok = (
  field: ViewField,
  opts: EmitOptions,
  visited: Set<string>,
): FieldTok => {
  let pair: { sample: string; next: string };
  if (field.kind === "primitive") {
    pair = samplesForNative(
      convertSpecType(field.base),
      field.base,
    );
  } else if (field.kind === "datasource") {
    const expr = renderDs(field.base, opts);
    pair = { sample: expr, next: expr };
  } else {
    const expr = renderViewExpr(field.base, opts, visited);
    pair = { sample: expr, next: expr };
  }
  return {
    ident: opts.naming.fieldName(field.name),
    sampleExpr: wrapValue(pair.sample, field),
    nextExpr: wrapValue(pair.next, field),
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
  return [
    { ident: "base", sampleExpr: base, nextExpr: base, nullable: false },
    ...declared,
  ];
};

const renderViewExpr = (
  name: string,
  opts: EmitOptions,
  visited: Set<string>,
): string => {
  if (visited.has(name)) throw new Error(`cyclic view reference: ${name}`);
  const view = opts.views.get(name);
  if (view === undefined) throw new Error(`unknown view: ${name}`);
  const next = new Set(visited).add(name);
  if (view.kind === "union") {
    const member = view.members[0];
    if (member === undefined) return `${qual(name, "view", opts.naming)} {}`;
    return `${qual(name, "view", opts.naming)}::${opts.naming.className(member)}(${renderViewExpr(member, opts, next)})`;
  }
  const body = shapedToks(view, opts, next)
    .map((t) => `${t.ident}: ${t.sampleExpr}`)
    .join(", ");
  return `${qual(name, "view", opts.naming)} { ${body} }`;
};

const fixtureExpr = (view: ShapedView, opts: EmitOptions, fields: FieldTok[]) => {
  const cls = opts.naming.className(view.name);
  if (fields.length === 0) return `${cls} {}`;
  const lines = fields.map((f) => `            ${f.ident}: ${f.sampleExpr},`);
  return `${cls} {\n${lines.join("\n")}\n        }`;
};

const testPath = (entity: string, naming: ArtifactPaths): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const renderTests = (view: ViewType, opts: EmitOptions): GenerateEntry => {
  const fields =
    view.kind === "shaped"
      ? shapedToks(view, opts, new Set([view.name]))
      : [];
  return content(
    testPath(view.name, opts.naming),
    fill(typeTestTmpl, {
      schemaVersion: opts.schemaVersion,
      structName: opts.naming.className(view.name),
      fileBase: opts.naming.fileBase(view.name),
      isShaped: view.kind === "shaped",
      isUnion: view.kind === "union",
      fixture:
        view.kind === "shaped" ? fixtureExpr(view, opts, fields) : "",
      fields,
      members:
        view.kind === "union"
          ? view.members.map((name) => ({
              ident: opts.naming.fieldName(name),
              memberExpr: `${opts.naming.className(view.name)}::${opts.naming.className(name)}(${renderViewExpr(name, opts, new Set([view.name]))})`,
            }))
          : [],
    }),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const base = emitBase(ctx.settings);
  const views = await new SpecificationParser(ctx.reader).loadViewTypes();
  const tables = (await ctx.reader.exists(DATASOURCE_TYPES_YAML))
    ? new SpecificationParser().parseDatasourceTypes({
        yaml: await ctx.reader.read(DATASOURCE_TYPES_YAML),
        idType: base.idType,
      })
    : [];
  const opts: EmitOptions = {
    ...base,
    tables: new Map(tables.map((t) => [t.name, t])),
    views: new Map(views.map((v) => [v.name, v])),
  };
  return views.map((view) => renderTests(view, opts));
};
