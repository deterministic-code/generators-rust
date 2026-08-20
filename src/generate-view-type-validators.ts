import { snakeCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { viewPaths, type ArtifactPaths } from "./common/paths.ts";
import { emitViewFields, inlinesParent, isAlias } from "./common/view-shape.ts";
import {
  DeterministicParser,
  VIEW_TYPES_YAML,
  type ShapedView,
  type ViewField,
  type ViewType,
  type IDeterministic,
} from "./specification-parser.ts";
import {
  checkArrayNullableTmpl,
  checkArrayTmpl,
  checkNullableTmpl,
  checkRequiredTmpl,
  typeTmpl,
} from "./resources/view-type-validators.ts";

type EmitOptions = {
  naming: ArtifactPaths;
  schemaVersion: string;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  naming: viewPaths(settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
});

const typePath = (
  entity: string,
  kind: "datasource" | "view",
  naming: ArtifactPaths,
): string => {
  const cls = naming.className(entity);
  if (naming.byFeature) {
    const module = naming
      .filePath(entity)
      .replace(/\.rs$/, "")
      .replace(/^features\//, "")
      .replaceAll("/", "::");
    return `crate::features::${module}::${cls}`;
  }
  const ns =
    kind === "datasource"
      ? "crate::types::generated::datasource"
      : "crate::types::generated::views";
  return `${ns}::${cls}`;
};

const validatorFn = (
  entity: string,
  kind: "datasource" | "view",
  naming: ArtifactPaths,
): string => {
  const fn =
    kind === "datasource"
      ? `validate_datasource_${snakeCase(entity)}`
      : `validate_${snakeCase(entity)}`;
  if (naming.byFeature) {
    const dir = naming
      .filePath(entity)
      .replace(/\/[^/]+$/, "")
      .replace(/^features\//, "")
      .replaceAll("/", "::");
    return `crate::features::${dir}::${naming.fileBase(entity)}_validator::${fn}`;
  }
  const ns =
    kind === "datasource"
      ? "crate::types::generated::datasource::validators"
      : "crate::types::generated::views::validators";
  return `${ns}::${fn}`;
};

const checkField = (field: ViewField, opts: EmitOptions): string => {
  const prop = opts.naming.fieldName(field.name);
  const access = `obj.${prop}`;
  if (field.kind === "primitive") return "";
  const fn = validatorFn(field.base, field.kind, opts.naming);
  if (field.isArray) {
    const tmpl = field.isNullable ? checkArrayNullableTmpl : checkArrayTmpl;
    return fill(tmpl, { access, fn }).trimEnd();
  }
  if (field.isNullable) {
    return fill(checkNullableTmpl, { access, fn }).trimEnd();
  }
  return fill(checkRequiredTmpl, { fn, arg: `&${access}` }).trimEnd();
};

const validatorPath = (entity: string, naming: ArtifactPaths): string =>
  naming.byFeature
    ? naming.filePath(entity).replace(/\.rs$/, "_validator.rs")
    : `${naming.fileBase(entity)}_validator.rs`;

const shapedBody = (
  view: ShapedView,
  expanded: ViewType | undefined,
  opts: EmitOptions,
) => {
  const checks: string[] = [];
  if (view.inherits !== null && !inlinesParent(view)) {
    const fn = validatorFn(view.inherits, "datasource", opts.naming);
    const arg = isAlias(view) ? "obj" : "&obj.base";
    checks.push(
      fill(checkRequiredTmpl, { fn, arg }).trimEnd(),
    );
  }
  for (const line of emitViewFields(view, expanded).map((f) =>
    checkField(f, opts),
  )) {
    if (line !== "") checks.push(line);
  }
  return checks;
};

const renderView = (
  view: ViewType,
  expanded: ViewType | undefined,
  opts: EmitOptions,
): GenerateEntry => {
  const fnName = `validate_${snakeCase(view.name)}`;
  const path = validatorPath(view.name, opts.naming);
  if (view.kind === "union") {
    const cls = typePath(view.name, "view", opts.naming);
    return content(
      path,
      fill(typeTmpl, {
        schemaVersion: opts.schemaVersion,
        isUnion: true,
        isShaped: false,
        fnName,
        typePath: cls,
        arms: view.members.map((m) => ({
          arm: `${cls}::${opts.naming.className(m)}(inner) => ${validatorFn(m, "view", opts.naming)}(inner),`,
        })),
        paramName: "obj",
        hasChecks: false,
        checks: [],
      }),
    );
  }
  const checks = shapedBody(view, expanded, opts);
  return content(
    path,
    fill(typeTmpl, {
      schemaVersion: opts.schemaVersion,
      isUnion: false,
      isShaped: true,
      fnName,
      typePath: typePath(view.name, "view", opts.naming),
      paramName: checks.length > 0 ? "obj" : "_obj",
      hasChecks: checks.length > 0,
      checks: checks.map((line) => ({ line })),
      arms: [],
    }),
  );
};

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const opts = emitOptions(settings);
  const expandedByName = new Map(
    deterministic.expandedViewTypes.map((v) => [v.name, v]),
  );
  return deterministic.viewTypes.map((view) =>
    renderView(view, expandedByName.get(view.name), opts),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(VIEW_TYPES_YAML);
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
    ctx.settings,
  );
};
