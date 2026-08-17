import type { SettingsDict } from "@deterministic-code/generator-sdk/settings-dict";
import { toCase, pascal, snake } from "@deterministic-code/generator-sdk/case";
import { testCasingOptionsFromSettings } from "@deterministic-code/generator-sdk/codegen/lib/generate-settings-options";
import { normalizeAll } from "@deterministic-code/generator-sdk/view-expand";
import type { RawTypesDoc } from "@deterministic-code/generator-sdk/deterministic-shapes";
import { viewGenerator } from "@deterministic-code/generator-sdk/codegen-context";
import { RustImports, rustImportsForOptions } from "./rust-imports.ts";
import {
  rustInjectedSystemFields,
  inlinedViewAuditFieldsExcluding,
  rustIdFieldValue,
} from "./rust-standard-columns.ts";
import {
  datasourceSettingsFor,
  type DatasourceOptions,
} from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import {
  classifyViewShape,
  declaredFieldsOf,
  type Datasource,
  type DeclaredField,
  type ShapedView,
  type UnionView,
  type View,
  type ViewField,
} from "@deterministic-code/generator-sdk/codegen/lib/generate-view-shared";
import type { DatasourceSettings } from "@deterministic-code/generator-sdk/datasource-settings";
import type { NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";

type Flatten<T> = { [K in keyof T]: T[K] };

type RustGenerateOptions = Flatten<
  NamesForOptions & DatasourceOptions & { schemaVersion: string }
>;

interface DsIndexEntry {
  name: string;
  declared: DeclaredField[];
  datasourceFields: DeclaredField[];
  inlinedFields: DeclaredField[];
}

interface RustImportRenderer {
  dsType(entity: string): string;
  viewType(entity: string): string;
  viewValidator(entity: string): string;
}

interface RustCtx {
  dsIndex: Map<string, DsIndexEntry>;
  viewIndex: Map<string, View>;
  visited: Set<string>;
  nullableVariant: boolean;
  imports: RustImportRenderer;
  ds: DatasourceSettings;
}

interface GeneratedFile {
  path: string;
  content: string;
}

export const DEFAULT_GENERATE_OPTIONS: RustGenerateOptions = {
  schemaVersion: "1.0",
};

function fileBase(name: string, opts: RustGenerateOptions): string {
  return toCase(`${name}_validator_tests`, opts.fileFormat!); // lint-generator-casing-allow: toCase
}

function indexDatasource(
  datasource: Datasource,
  ds: DatasourceSettings,
): Map<string, DsIndexEntry> {
  const index = new Map<string, DsIndexEntry>();
  for (const entry of datasource.types ?? []) {
    const [name, def] = Object.entries(entry)[0];
    const declared = declaredFieldsOf(def);
    const declaredNames = new Set(declared.map((f) => f.name));
    index.set(name, {
      name,
      declared,
      datasourceFields: [
        ...(rustInjectedSystemFields(def, ds) as DeclaredField[]),
        ...declared,
      ],
      inlinedFields: [
        ...(inlinedViewAuditFieldsExcluding(
          declaredNames,
          ds,
        ) as DeclaredField[]),
        ...declared,
      ],
    });
  }
  return index;
}

function primitiveExpr(base: string, useNext: boolean): string {
  switch (base) {
    case "string":
      return useNext ? `String::from("next")` : `String::from("sample")`;
    case "uuid":
      return `uuid::Uuid::new_v4().to_string()`;
    case "number":
    case "reference":
      return useNext ? `42i64` : `1i64`;
    case "boolean":
      return useNext ? `false` : `true`;
    case "datetime":
      return useNext
        ? `chrono::Utc::now() + chrono::Duration::days(1)`
        : `chrono::Utc::now()`;
    case "binary":
      return useNext ? `vec![1u8]` : `Vec::<u8>::new()`;
    default:
      return `Default::default()`;
  }
}

function renderDatasourceField(
  field: DeclaredField,
  ctx: RustCtx,
  useNext: boolean,
): string {
  if (field.isNullable && ctx.nullableVariant) return "None";
  const base =
    field.name === "id"
      ? rustIdFieldValue(ctx.ds, useNext ? "next" : "sample")
      : primitiveExpr(field.type, useNext);
  return field.isNullable ? `Some(${base})` : base;
}

function renderDatasourceStruct(name: string, ctx: RustCtx): string {
  const def = ctx.dsIndex.get(name);
  if (!def) throw new Error(`unknown datasource type: ${name}`);
  const cls = ctx.imports.dsType(name);
  const lines = def.datasourceFields.map((f) => {
    const val = renderDatasourceField(f, ctx, false);
    return `        ${snake(f.name)}: ${val},`; // lint-generator-casing-allow: snake
  });
  if (lines.length === 0) return `${cls} {}`;
  return [`${cls} {`, ...lines, `    }`].join("\n");
}

function renderViewFieldValue(
  field: ViewField,
  ctx: RustCtx,
  useNext: boolean,
): string {
  const { parsed } = field;
  const makeElement = (): string => {
    if (parsed.kind === "primitive") {
      return primitiveExpr(parsed.base, useNext);
    }
    if (parsed.kind === "datasource") {
      return renderDatasourceStruct(parsed.base, ctx);
    }
    return renderViewStruct(parsed.base, ctx);
  };

  if (parsed.isArray) {
    if (field.isNullable && ctx.nullableVariant) return "None";
    const elem = makeElement();
    const arr = `vec![${elem}]`;
    return field.isNullable ? `Some(${arr})` : arr;
  }

  if (field.isNullable && ctx.nullableVariant) return "None";
  const value = makeElement();
  return field.isNullable ? `Some(${value})` : value;
}

function pushFieldLine(args: {
  lines: string[];
  generated: Set<string>;
  viewName: string;
  fieldName: string;
  line: string;
}): void {
  const { lines, generated, viewName, fieldName, line } = args;
  if (generated.has(fieldName)) {
    throw new Error(
      `generate-view-tests-rust: duplicate field "${fieldName}" in struct literal for view "${viewName}". ` +
        `This indicates a field-name collision the validator should have caught (likely a view declares a ` +
        `field with the same name as one of its inherited datasource's fields, or an auto-injected ` +
        `column was unintentionally re-generated by codegen). Fix the validator gate or the view's YAML.`,
    );
  }
  generated.add(fieldName);
  lines.push(line);
}

/** Push the inherited datasource's inlined fields (minus `omit`) as struct-literal lines. */
function pushInlinedParentFields(args: {
  lines: string[];
  generated: Set<string>;
  name: string;
  ctx: RustCtx;
  inherits: string;
  omit: Set<string>;
}): void {
  const { lines, generated, name, ctx, inherits, omit } = args;
  const def = ctx.dsIndex.get(inherits);
  if (!def) return;
  for (const f of def.inlinedFields) {
    if (omit.has(f.name)) continue;
    const val = renderDatasourceField(f, ctx, false);
    pushFieldLine({
      lines,
      generated,
      viewName: name,
      fieldName: f.name,
      line: `        ${snake(f.name)}: ${val},`, // lint-generator-casing-allow: snake
    });
  }
}

/** The struct-literal field lines for a shaped view: inherited (inlined/omit/base) columns then declared fields. */
function viewStructFieldLines(args: {
  view: ShapedView;
  name: string;
  ctx: RustCtx;
  shape: ReturnType<typeof classifyViewShape>;
}): string[] {
  const { view, name, ctx, shape } = args;
  const { enrichments, inlineParent, inlineForOmit, omitList } = shape;
  const lines: string[] = [];
  const generated = new Set<string>();
  const inlined = (omit: Set<string>): void =>
    pushInlinedParentFields({
      lines,
      generated,
      name,
      ctx,
      inherits: view.inherits!,
      omit,
    });
  if (view.inherits && inlineParent) {
    inlined(new Set(enrichments.map((e) => e.fkColumn)));
  } else if (view.inherits && inlineForOmit) {
    inlined(new Set(omitList));
  } else if (view.inherits) {
    lines.push(`        base: ${renderDatasourceStruct(view.inherits, ctx)},`);
    generated.add("base");
  }
  for (const f of view.fields) {
    const val = renderViewFieldValue(f, ctx, false);
    pushFieldLine({
      lines,
      generated,
      viewName: name,
      fieldName: f.name,
      line: `        ${snake(f.name)}: ${val},`, // lint-generator-casing-allow: snake
    });
  }
  return lines;
}

function renderViewStruct(name: string, ctx: RustCtx): string {
  if (ctx.visited.has(`view:${name}`)) {
    throw new Error(`cyclic view reference: ${name}`);
  }
  const view = ctx.viewIndex.get(name);
  if (!view) throw new Error(`unknown view: ${name}`);
  ctx.visited.add(`view:${name}`);
  try {
    if (view.kind === "union") {
      const memberExpr = renderViewStruct(view.members[0], ctx);
      return `${ctx.imports.viewType(name)}::${pascal(view.members[0])}(${memberExpr})`; // lint-generator-casing-allow: pascal
    }
    const shape = classifyViewShape(view);
    if (shape.aliasParent) return renderDatasourceStruct(view.inherits!, ctx);
    const cls = ctx.imports.viewType(name);
    const lines = viewStructFieldLines({ view, name, ctx, shape });
    if (lines.length === 0) return `${cls} {}`;
    return [`${cls} {`, ...lines, `    }`].join("\n");
  } finally {
    ctx.visited.delete(`view:${name}`);
  }
}

function hasAnyNullable(view: ShapedView): boolean {
  return view.fields.some((f) => f.isNullable);
}

function renderShapedTests(view: ShapedView, ctx: RustCtx): string[] {
  const validExpr = renderViewStruct(view.name, {
    ...ctx,
    nullableVariant: false,
  });
  const tests = [
    [
      `    #[test]`,
      `    fn parses_a_valid_payload() {`,
      `        let value = ${validExpr};`,
      `        assert!(${ctx.imports.viewValidator(view.name)}(&value).is_ok());`,
      `    }`,
    ].join("\n"),
  ];

  if (hasAnyNullable(view)) {
    const nullableExpr = renderViewStruct(view.name, {
      ...ctx,
      nullableVariant: true,
    });
    tests.push(
      [
        `    #[test]`,
        `    fn accepts_null_for_nullable_fields() {`,
        `        let value = ${nullableExpr};`,
        `        assert!(${ctx.imports.viewValidator(view.name)}(&value).is_ok());`,
        `    }`,
      ].join("\n"),
    );
  }

  tests.push(...fieldMutationTests({ view, validExpr, ctx }));
  return tests;
}

/** Per-field `gets_and_sets_*` mutation tests, plus `allows_setting_*_to_none` for each nullable field. */
function fieldMutationTests(args: {
  view: ShapedView;
  validExpr: string;
  ctx: RustCtx;
}): string[] {
  const { view, validExpr, ctx } = args;
  const tests: string[] = [];
  for (const field of view.fields) {
    const name = snake(field.name); // lint-generator-casing-allow: snake
    const nextExpr = renderViewFieldValue(
      field,
      { ...ctx, nullableVariant: false },
      true,
    );
    tests.push(
      [
        `    #[test]`,
        `    fn gets_and_sets_${name}() {`,
        `        let mut value = ${validExpr};`,
        `        let next = ${nextExpr};`,
        `        value.${name} = next.clone();`,
        `        assert_eq!(value.${name}, next);`,
        `    }`,
      ].join("\n"),
    );
  }
  for (const field of view.fields) {
    if (!field.isNullable) continue;
    const name = snake(field.name); // lint-generator-casing-allow: snake
    tests.push(
      [
        `    #[test]`,
        `    fn allows_setting_${name}_to_none() {`,
        `        let mut value = ${validExpr};`,
        `        value.${name} = None;`,
        `        assert!(value.${name}.is_none());`,
        `    }`,
      ].join("\n"),
    );
  }
  return tests;
}

function renderUnionTests(view: UnionView, ctx: RustCtx): string[] {
  const tests: string[] = [];

  for (const member of view.members) {
    const memberFn = `accepts_${snake(member)}_member`; // lint-generator-casing-allow: snake
    const memberExpr = `${ctx.imports.viewType(view.name)}::${pascal(member)}(${renderViewStruct(member, ctx)})`; // lint-generator-casing-allow: pascal
    tests.push(
      [
        `    #[test]`,
        `    fn ${memberFn}() {`,
        `        let value = ${memberExpr};`,
        `        assert!(${ctx.imports.viewValidator(view.name)}(&value).is_ok());`,
        `    }`,
      ].join("\n"),
    );
  }

  return tests;
}

export function generateForView(
  view: View,
  {
    datasource,
    viewIndex,
  }: { datasource: Datasource; viewIndex: Map<string, View> },
  options: Partial<RustGenerateOptions> = {},
): GeneratedFile {
  const opts: RustGenerateOptions = { ...DEFAULT_GENERATE_OPTIONS, ...options };
  const ds = datasourceSettingsFor(opts);
  const ctx: RustCtx = {
    dsIndex: indexDatasource(datasource, ds),
    viewIndex,
    visited: new Set(),
    nullableVariant: false,
    imports: rustImportsForOptions(opts),
    ds,
  };
  const tests =
    view.kind === "union"
      ? renderUnionTests(view, ctx)
      : renderShapedTests(view, ctx);

  const header = [
    `// schema-version: ${opts.schemaVersion}`,
    `#[cfg(test)]`,
    `mod tests {`,
    ``,
  ].join("\n");

  const body = `${tests.join("\n\n")}\n}\n`;
  return {
    path: `${fileBase(view.name, opts)}.rs`,
    content: `${header}${body}`,
  };
}

export function generateFromSchema(
  { viewTypes, datasource }: { viewTypes: unknown; datasource: Datasource },
  options: Partial<RustGenerateOptions> = DEFAULT_GENERATE_OPTIONS,
): GeneratedFile[] {
  const opts: RustGenerateOptions = { ...DEFAULT_GENERATE_OPTIONS, ...options };
  const normalized = normalizeAll(viewTypes as RawTypesDoc) as View[];
  const viewIndex = new Map(normalized.map((v) => [v.name, v]));
  return normalized.map((v) => generateForView(v, { datasource, viewIndex }, opts));
}

const baseCreateGenerator = viewGenerator((view, ctx) => {
  const viewIndex = (ctx.viewTestIndex ??= new Map(
    normalizeAll(ctx.opts.viewTypes).map((v: View) => [v.name, v]),
  ));
  const file = generateForView(
    view,
    { datasource: ctx.opts.datasourceTypes, viewIndex },
    ctx.opts,
  );
  if (!ctx.byFeature) return file;
  const fileName = `${ctx.names.fileBase(view.name, "view-validator")}_tests${ctx.names.ext}`;
  return {
    ...file,
    path: ctx.layout.testPath(view.name, "view-validator", { fileName }),
  };
});

export const createGenerator = () => {
  const base = baseCreateGenerator(RustImports);
  return {
    generate: (config: { settings: SettingsDict; language: string }) =>
      base.generate({
        ...DEFAULT_GENERATE_OPTIONS,
        ...testCasingOptionsFromSettings(config),
        ...config,
      }),
  };
};
