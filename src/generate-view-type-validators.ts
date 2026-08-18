import { snakeCase } from "change-case";
import {
  datasourceSettings,
  type DatasourceSettings,
} from "./common/datasource-settings.ts";
import { fill } from "./common/fill.ts";
import type { GenerateContext, SettingsDict } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import { rustNaming, type ArtifactNaming } from "./common/naming.ts";
import {
  loadViewTypes,
  type ShapedView,
  type ViewField,
  type ViewType,
} from "./common/parse-view-types.ts";
import { settingsStr } from "./common/settings.ts";
import { typeTmpl } from "./resources/view-type-validators.ts";

type EmitOptions = {
  ds: DatasourceSettings;
  naming: ArtifactNaming;
  schemaVersion: string;
};

const emitOptions = (settings: SettingsDict): EmitOptions => ({
  ds: datasourceSettings(settings),
  naming: rustNaming(settings),
  schemaVersion: settingsStr(settings, "codegen.schema_version") ?? "1.0",
});

const typePath = (
  entity: string,
  kind: "datasource" | "view",
  naming: ArtifactNaming,
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
  naming: ArtifactNaming,
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

const inlinesParent = (view: ShapedView): boolean =>
  view.inherits !== null &&
  (view.enrichments.length > 0 || view.omit.length > 0);

const isAlias = (view: ShapedView): boolean =>
  view.inherits !== null &&
  view.fields.length === 0 &&
  view.enrichments.length === 0 &&
  view.omit.length === 0;

const checkField = (field: ViewField, opts: EmitOptions): string => {
  const prop = opts.naming.fieldName(field.name);
  const access = `obj.${prop}`;
  if (field.kind === "primitive") return "";
  const fn = validatorFn(field.base, field.kind, opts.naming);
  if (field.isArray) {
    if (field.isNullable) {
      return `    if let Some(arr) = &${access} { for item in arr { if let Err(mut e) = ${fn}(item) { errors.append(&mut e); } } }`;
    }
    return `    for item in &${access} { if let Err(mut e) = ${fn}(item) { errors.append(&mut e); } }`;
  }
  if (field.isNullable) {
    return `    if let Some(inner) = &${access} { if let Err(mut e) = ${fn}(inner) { errors.append(&mut e); } }`;
  }
  return `    if let Err(mut e) = ${fn}(&${access}) { errors.append(&mut e); }`;
};

const validatorPath = (entity: string, naming: ArtifactNaming): string =>
  naming.byFeature
    ? naming.filePath(entity).replace(/\.rs$/, "_validator.rs")
    : `${naming.fileBase(entity)}_validator.rs`;

const shapedBody = (view: ShapedView, opts: EmitOptions) => {
  const checks: string[] = [];
  if (view.inherits !== null && !inlinesParent(view)) {
    const fn = validatorFn(view.inherits, "datasource", opts.naming);
    const arg = isAlias(view) ? "obj" : "&obj.base";
    checks.push(
      `    if let Err(mut e) = ${fn}(${arg}) { errors.append(&mut e); }`,
    );
  }
  for (const line of view.fields.map((f) => checkField(f, opts))) {
    if (line !== "") checks.push(line);
  }
  return checks;
};

const renderView = (view: ViewType, opts: EmitOptions): GenerateEntry => {
  const fnName =
    view.kind === "union"
      ? `validate_${snakeCase(view.name)}`
      : `validate_${snakeCase(view.name)}`;
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
  const checks = shapedBody(view, opts);
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

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const opts = emitOptions(ctx.settings);
  const views = await loadViewTypes(ctx.reader);
  return views.map((view) => renderView(view, opts));
};
