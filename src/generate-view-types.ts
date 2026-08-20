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
import { typeTmpl } from "./resources/view-types.ts";

const docTokens = (settings: Record<string, string>) => {
  const comments = settings["comments"];
  return {
    simpleDoc: comments !== "none" && comments !== "description",
    descriptionDoc: comments === "description",
  };
};

type EmitOptions = {
  idType: string;
  naming: ArtifactPaths;
  schemaVersion: string;
  simpleDoc: boolean;
  descriptionDoc: boolean;
  tables: Map<string, DatasourceType>;
};

const emitBase = (settings: Record<string, string>) => ({
  idType: settings["datasource.id_type"] ?? "integer",
  naming: viewPaths(settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
  ...docTokens(settings),
});

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
      ? convertSpecType(field.base)
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
  return tableFields(table.fields, opts.idType)
    .filter((f) => !omit.has(f.name))
    .map((f) => {
      const native = convertSpecType(f.type);
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
      simpleDoc: opts.simpleDoc,
      descriptionDoc: opts.descriptionDoc,
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
  };
  return views.map((view) => renderView(view, opts));
};
