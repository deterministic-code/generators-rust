import { pascalCase, snakeCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  createImportGenerator,
  type RustImportGenerator,
} from "./import-generator.ts";
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
  imports: RustImportGenerator;
  schemaVersion: string;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  imports: createImportGenerator(".", settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
});

const className = (entity: string): string => pascalCase(entity);
const fieldName = (field: string): string => field;

const typePath = (
  entity: string,
  kind: "datasource" | "view",
  imports: RustImportGenerator,
): string =>
  kind === "datasource"
    ? imports.datasourceQual(entity)
    : imports.viewQual(entity);

const validatorFn = (
  entity: string,
  kind: "datasource" | "view",
  imports: RustImportGenerator,
): string => {
  const fn =
    kind === "datasource"
      ? `validate_datasource_${snakeCase(entity)}`
      : `validate_${snakeCase(entity)}`;
  return imports.validatorFn(kind, entity, fn);
};

const checkField = (field: ViewField, opts: EmitOptions): string => {
  const prop = fieldName(field.name);
  const access = `obj.${prop}`;
  if (field.kind === "primitive") return "";
  const fn = validatorFn(field.base, field.kind, opts.imports);
  if (field.isArray) {
    const tmpl = field.isNullable ? checkArrayNullableTmpl : checkArrayTmpl;
    return fill(tmpl, { access, fn }).trimEnd();
  }
  if (field.isNullable) {
    return fill(checkNullableTmpl, { access, fn }).trimEnd();
  }
  return fill(checkRequiredTmpl, { fn, arg: `&${access}` }).trimEnd();
};

const validatorPath = (entity: string, imports: RustImportGenerator): string =>
  imports.viewValidator(entity);

const shapedBody = (
  view: ShapedView,
  expanded: ViewType | undefined,
  opts: EmitOptions,
) => {
  const checks: string[] = [];
  if (view.inherits !== null && !inlinesParent(view)) {
    const fn = validatorFn(view.inherits, "datasource", opts.imports);
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
  const path = validatorPath(view.name, opts.imports);
  if (view.kind === "union") {
    const cls = typePath(view.name, "view", opts.imports);
    return content(
      path,
      fill(typeTmpl, {
        schemaVersion: opts.schemaVersion,
        isUnion: true,
        isShaped: false,
        fnName,
        typePath: cls,
        arms: view.members.map((m) => ({
          arm: `${cls}::${className(m)}(inner) => ${validatorFn(m, "view", opts.imports)}(inner),`,
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
      typePath: typePath(view.name, "view", opts.imports),
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
