import {
  DEFAULT_COMMENT_STYLE,
  renderDocComment,
} from "@deterministic-code/generator-sdk/generate-doc-comment";
import { inlinedViewAuditFieldsExcluding } from "./rust-standard-columns.ts";
import { viewGenerator } from "@deterministic-code/generator-sdk/codegen-context";
import { RustImports } from "./rust-imports.ts";
import { datasourceOptionsFromSettings } from "@deterministic-code/generator-sdk/codegen/lib/generate-settings-options";
import {
  datasourceSettingsFor,
  type DatasourceOptions,
} from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import type { DatasourceSettings } from "@deterministic-code/generator-sdk/datasource-settings";
import {
  buildViewGenerator,
  classifyViewShape,
  declaredFieldsOf,
  shapedViewDocLines,
  unionViewDocLines,
  type DatasourceTypeDef,
  type DeclaredField,
  type ShapedView,
  type View,
  type ViewField,
} from "@deterministic-code/generator-sdk/codegen/lib/generate-view-shared";
import { createTypeMapper } from "@deterministic-code/generator-sdk/codegen/lib/type-mapper";

const mapRustType = createTypeMapper("rust");

export const DEFAULT_GENERATE_OPTIONS = {
  baseClass: null,
  schemaVersion: "1.0",
  style: DEFAULT_COMMENT_STYLE,
};

interface RustDatasource {
  types: Array<Record<string, DatasourceTypeDef>>;
}

interface RustGenerateOpts extends DatasourceOptions {
  datasource: RustDatasource;
  schemaVersion: string;
  style: unknown;
}

interface RustImportRenderer {
  glob(): string;
  qualified(base: string): string;
  viewType(base: string): string;
}

interface RustCtx {
  names: { className(n: string): string };
  fields: { name(n: string): string };
  opts: RustGenerateOpts;
  imports: RustImportRenderer;
  layout: { filePath(n: string, a: string): string };
}

function inheritedDatasourceFields(
  datasource: RustDatasource,
  tableName: string,
  ds: DatasourceSettings,
): DeclaredField[] {
  const entry = datasource.types.find((e) => Object.keys(e)[0] === tableName);
  if (!entry) {
    const available = datasource.types.map((e) => Object.keys(e)[0]).join(", ");
    throw new Error(
      `generate-view: view inherits datasource_types.${tableName} but no such type in datasource. Available: ${available || "(none)"}.`,
    );
  }
  const def = Object.values(entry)[0];
  const declared = declaredFieldsOf(def);
  const declaredNames = new Set(declared.map((f) => f.name));
  return [...inlinedViewAuditFieldsExcluding(declaredNames, ds), ...declared];
}

function generateInlinedField(field: DeclaredField, ctx: RustCtx): string {
  const rustType =
    field.name === "id"
      ? datasourceSettingsFor(ctx.opts).rustIdType()
      : mapRustType(field.type);
  const wrapped = field.isNullable ? `Option<${rustType}>` : rustType;
  return `    pub ${ctx.fields.name(field.name)}: ${wrapped},`;
}

function rustTypeForField(field: ViewField, ctx: RustCtx): string {
  const { parsed } = field;
  let base: string;
  switch (parsed.kind) {
    case "primitive":
      base = mapRustType(parsed.base);
      break;
    case "datasource":
      base = ctx.imports.qualified(parsed.base);
      break;
    case "view":
      base = ctx.imports.viewType(parsed.base);
      break;
    default:
      throw new Error(`Unknown field kind: ${parsed.kind}`);
  }
  const arr = parsed.isArray ? `Vec<${base}>` : base;
  return field.isNullable ? `Option<${arr}>` : arr;
}

function generateField(field: ViewField, ctx: RustCtx): string {
  const type = rustTypeForField(field, ctx);
  return `    pub ${ctx.fields.name(field.name)}: ${type},`;
}

function inlineFieldsBlock(
  view: ShapedView,
  omit: Set<string>,
  ctx: RustCtx,
): string {
  const parentFields = inheritedDatasourceFields(
    ctx.opts.datasource,
    view.inherits!,
    datasourceSettingsFor(ctx.opts),
  ).filter((f) => !omit.has(f.name));
  const lines = [
    ...parentFields.map((f) => generateInlinedField(f, ctx)),
    ...view.fields.map((f) => generateField(f, ctx)),
  ];
  return `${lines.join("\n")}\n`;
}

interface ShapeFlags {
  enrichments: { fkColumn: string }[];
  inlineParent: unknown;
  inlineForOmit: unknown;
  omitList: string[];
}

/** The struct field lines for a shaped view: inlined-parent / omit-inlined columns, or the `base` alias + declared fields. */
function viewFieldsBlock(
  view: ShapedView,
  ctx: RustCtx,
  { enrichments, inlineParent, inlineForOmit, omitList }: ShapeFlags,
): string {
  const { imports } = ctx;
  if (inlineParent) {
    return inlineFieldsBlock(
      view,
      new Set(enrichments.map((e) => e.fkColumn)),
      ctx,
    );
  }
  if (inlineForOmit) {
    return inlineFieldsBlock(view, new Set(omitList), ctx);
  }
  const baseField = view.inherits
    ? `    pub base: ${imports.qualified(view.inherits)},\n`
    : "";
  const body = view.fields.map((f) => generateField(f, ctx)).join("\n");
  const bodyBlock = body ? `${body}\n` : "";
  return `${baseField}${bodyBlock}`;
}

function renderView(view: View, ctx: RustCtx) {
  const { names, opts, imports, layout } = ctx;
  const structName = names.className(view.name);
  const path = layout.filePath(view.name, "view-type");
  const header = `// schema-version: ${opts.schemaVersion}\n${imports.glob()}`;
  const derives = `#[derive(Clone, Debug, PartialEq)]\n`;

  if (view.kind === "union") {
    const variants = view.members
      .map((m) => {
        const variant = names.className(m);
        return `    ${variant}(${imports.viewType(m)}),`;
      })
      .join("\n");
    const doc = renderDocComment({
      style: opts.style,
      summary: `View ${structName}.`,
      lines: unionViewDocLines(view),
      language: "rust",
    });
    const content = `${header}${doc}${derives}pub enum ${structName} {\n${variants}\n}\n`;
    return { path, content };
  }

  const { enrichments, inlineParent, inlineForOmit, omitList, aliasParent } =
    classifyViewShape(view);

  if (aliasParent) {
    const qualified = imports.qualified(view.inherits!);
    const doc = renderDocComment({
      style: opts.style,
      summary: `View ${structName}.`,
      lines: [
        `Datasource type: ${view.inherits}.`,
        `Target: ShapedView.`,
        `Fields: 0.`,
      ],
      language: "rust",
    });
    const content = `${header}${doc}pub type ${structName} = ${qualified};\n`;
    return { path, content };
  }

  const fieldsBlock = viewFieldsBlock(view, ctx, {
    enrichments,
    inlineParent,
    inlineForOmit,
    omitList,
  });
  const doc = renderDocComment({
    style: opts.style,
    summary: `View ${structName}.`,
    lines: shapedViewDocLines(view),
    language: "rust",
  });
  const content = `${header}${doc}${derives}pub struct ${structName} {\n${fieldsBlock}}\n`;
  return { path, content };
}

const baseCreateGenerator = viewGenerator(renderView);

/** Generator owns its options: DEFAULT_GENERATE_OPTIONS + datasource repr from settings; casing from CodegenNames; imports via RustImports. */
export const createGenerator = () =>
  buildViewGenerator({
    baseCreateGenerator,
    imports: RustImports,
    defaults: DEFAULT_GENERATE_OPTIONS,
    optionsFromSettings: datasourceOptionsFromSettings,
  });
