import {
  datasourceSettings,
  nativeFieldType,
  tableFields,
  type DatasourceSettings,
} from "./common/datasource-settings.ts";
import { commentStyle, type CommentStyle } from "./common/doc-comment.ts";
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
import { typeTmpl } from "./view-types/resources.ts";

type EmitOptions = {
  ds: DatasourceSettings;
  naming: ArtifactNaming;
  schemaVersion: string;
  style: CommentStyle;
  tables: Map<string, DatasourceType>;
};

const emitBase = (settings: SettingsDict) => ({
  ds: datasourceSettings(settings),
  naming: rustNaming(settings),
  schemaVersion: settingsStr(settings, "codegen.schema_version") ?? "1.0",
  style: commentStyle(settingsStr(settings, "comments")),
});

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

const inlinesParent = (view: ShapedView): boolean =>
  view.inherits !== null &&
  (view.enrichments.length > 0 || view.omit.length > 0);

const isAlias = (view: ShapedView): boolean =>
  view.inherits !== null &&
  view.fields.length === 0 &&
  view.enrichments.length === 0 &&
  view.omit.length === 0;

const rustTypeFor = (field: ViewField, opts: EmitOptions): string => {
  let base =
    field.kind === "primitive"
      ? convertSpecType(field.base, opts.ds.datetimeRepr)
      : qual(field.base, field.kind, opts.naming);
  if (field.isArray) base = `Vec<${base}>`;
  return field.isNullable ? `Option<${base}>` : base;
};

const parentFields = (view: ShapedView, opts: EmitOptions) => {
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
      const native = nativeFieldType(opts.ds, f);
      const rustType = f.isNullable ? `Option<${native}>` : native;
      return { ident: opts.naming.fieldName(f.name), rustType };
    });
};

const structFields = (view: ShapedView, opts: EmitOptions) => {
  const declared = view.fields.map((f) => ({
    ident: opts.naming.fieldName(f.name),
    rustType: rustTypeFor(f, opts),
  }));
  if (view.inherits === null) return declared;
  if (isAlias(view)) return [];
  if (inlinesParent(view)) return [...parentFields(view, opts), ...declared];
  return [
    {
      ident: "base",
      rustType: qual(view.inherits, "datasource", opts.naming),
    },
    ...declared,
  ];
};

const renderView = (view: ViewType, opts: EmitOptions): GenerateEntry => {
  const structName = opts.naming.className(view.name);
  const isUnion = view.kind === "union";
  const alias = !isUnion && isAlias(view);
  const isStruct = !isUnion && !alias;
  const fields = isUnion ? [] : structFields(view, opts);
  return content(
    opts.naming.filePath(view.name),
    fill(typeTmpl, {
      schemaVersion: opts.schemaVersion,
      simpleDoc: opts.style === "simple",
      descriptionDoc: opts.style === "description",
      structName,
      datasourceType: isUnion ? "standard" : (view.inherits ?? "standard"),
      target: isUnion ? "UnionView" : "ShapedView",
      fieldCount: String(
        isUnion ? view.members.length : isStruct ? fields.length : 0,
      ),
      isAlias: alias,
      aliasType:
        !isUnion && view.inherits !== null
          ? qual(view.inherits, "datasource", opts.naming)
          : "",
      isUnion,
      isStruct,
      members: isUnion
        ? view.members.map((m) => ({
            variant: opts.naming.className(m),
            memberType: qual(m, "view", opts.naming),
          }))
        : [],
      fields,
    }),
  );
};

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
  };
  return views.map((view) => renderView(view, opts));
};
