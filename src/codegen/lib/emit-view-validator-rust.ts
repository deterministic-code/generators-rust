import { toCase } from "@deterministic-code/generator-sdk/case";
import {
  buildViewEmitter,
  classifyViewShape,
  renderByViewKind,
  type ParsedField,
  type ShapedView,
  type UnionView,
  type View,
  type ViewField,
} from "@deterministic-code/generator-sdk/codegen/lib/emit-view-shared";
import { viewEmitter } from "@deterministic-code/generator-sdk/codegen-context";
import { RustImports } from "./rust-imports.ts";
import { datetimeOptionFromSettings } from "@deterministic-code/generator-sdk/codegen/lib/emit-settings-options";

export const DEFAULT_EMIT_OPTIONS = {
  schemaVersion: "1.0",
};

interface RustValidatorImports {
  dsValidator(base: string): string;
  viewValidator(base: string): string;
  viewType(name: string): string;
}

interface RustValidatorCtx {
  names: { fileBase(n: string, a: string): string; ext: string };
  fields: { name(n: string): string };
  opts: { schemaVersion: string };
  imports: RustValidatorImports;
  layout: { filePath(n: string, a: string): string };
  byFeature: boolean;
}

function viewValidatorFn(name: string): string {
  return `validate_${toCase(name, "Snake")}`; // lint-emitter-casing-allow: toCase
}

/** Fully-qualified path to the nested validator fn a field's element type needs, resolved through the injected rust import handler. */
function nestedFnFor(parsed: ParsedField, ctx: RustValidatorCtx): string {
  if (parsed.kind === "datasource") return ctx.imports.dsValidator(parsed.base);
  return ctx.imports.viewValidator(parsed.base);
}

function validateField(field: ViewField, ctx: RustValidatorCtx): string {
  const prop = ctx.fields.name(field.name);
  const access = `obj.${prop}`;
  const errs = "errors";

  if (field.parsed.isArray) {
    if (field.parsed.kind === "primitive") return "";
    const fn = nestedFnFor(field.parsed, ctx);
    if (field.isNullable) {
      return `    if let Some(arr) = &${access} { for item in arr { if let Err(mut e) = ${fn}(item) { ${errs}.append(&mut e); } } }\n`;
    }
    return `    for item in &${access} { if let Err(mut e) = ${fn}(item) { ${errs}.append(&mut e); } }\n`;
  }
  if (field.parsed.kind === "primitive") return "";
  const fn = nestedFnFor(field.parsed, ctx);
  if (field.isNullable) {
    return `    if let Some(inner) = &${access} { if let Err(mut e) = ${fn}(inner) { ${errs}.append(&mut e); } }\n`;
  }
  return `    if let Err(mut e) = ${fn}(&${access}) { ${errs}.append(&mut e); }\n`;
}

function emitShapedValidator(view: ShapedView, ctx: RustValidatorCtx): string {
  const cls = ctx.imports.viewType(view.name);
  const fn = viewValidatorFn(view.name);
  const { inlineParent, inlineForOmit, aliasParent } = classifyViewShape(view);
  const parentArg = aliasParent ? "obj" : "&obj.base";
  const parentCall =
    view.inherits && !inlineParent && !inlineForOmit
      ? `    if let Err(mut e) = ${ctx.imports.dsValidator(view.inherits)}(${parentArg}) { errors.append(&mut e); }\n`
      : "";
  const body = view.fields.map((f) => validateField(f, ctx)).join("");
  if (parentCall === "" && body === "") {
    return [
      `pub fn ${fn}(_obj: &${cls}) -> Result<(), Vec<String>> {`,
      `    Ok(())`,
      `}`,
    ].join("\n");
  }
  return [
    `pub fn ${fn}(obj: &${cls}) -> Result<(), Vec<String>> {`,
    `    let mut errors: Vec<String> = Vec::new();`,
    `${parentCall}${body}    if errors.is_empty() { Ok(()) } else { Err(errors) }`,
    `}`,
  ].join("\n");
}

function emitUnionValidator(view: UnionView, ctx: RustValidatorCtx): string {
  const cls = ctx.imports.viewType(view.name);
  const fn = viewValidatorFn(view.name);
  const arms = view.members
    .map((m) => {
      const variant = toCase(m, "Pascal"); // lint-emitter-casing-allow: toCase
      return `        ${cls}::${variant}(inner) => ${ctx.imports.viewValidator(m)}(inner),`;
    })
    .join("\n");
  return [
    `pub fn ${fn}(obj: &${cls}) -> Result<(), Vec<String>> {`,
    `    match obj {`,
    arms,
    `    }`,
    `}`,
  ].join("\n");
}

function renderView(view: View, ctx: RustValidatorCtx) {
  const { names, opts } = ctx;
  const header = `// schema-version: ${opts.schemaVersion}\n`;
  const body = renderByViewKind<View, RustValidatorCtx, string>(view, ctx, {
    shaped: (v, c) => emitShapedValidator(v as ShapedView, c),
    union: (v, c) => emitUnionValidator(v as UnionView, c),
  });
  const flatName = `${names.fileBase(view.name, "view-validator")}_validator${names.ext}`;
  const path = ctx.byFeature
    ? ctx.layout.filePath(view.name, "view-validator")
    : flatName;
  return {
    path,
    content: `${header}${body}\n`,
  };
}

const baseCreateEmitter = viewEmitter(renderView);

/** Emitter owns its options: DEFAULT_EMIT_OPTIONS + datetime from settings; casing from CodegenNames; nested-validator paths via RustImports. */
export const createEmitter = () =>
  buildViewEmitter({
    baseCreateEmitter,
    imports: RustImports,
    defaults: DEFAULT_EMIT_OPTIONS,
    optionsFromSettings: datetimeOptionFromSettings,
  });
