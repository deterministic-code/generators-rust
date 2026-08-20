import type { ArtifactPaths } from "./paths.ts";
import { inlinesParent, isAlias } from "./view-shape.ts";
import { sampleForField, samplesForNative } from "./test-samples.ts";
import { convertSpecType } from "../base-type-converter.ts";
import type {
  DatasourceType,
  ShapedView,
  ViewField,
  ViewType,
} from "../specification-parser.ts";

export type ViewTestOpts = {
  naming: ArtifactPaths;
  tables: Map<string, DatasourceType>;
  views: Map<string, ViewType>;
  expandedViews: Map<string, ViewType>;
};

export type FieldTok = {
  ident: string;
  sampleExpr: string;
  nextExpr: string;
  nullable: boolean;
};

export const qual = (
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

export const renderDs = (name: string, opts: ViewTestOpts): string => {
  const table = opts.tables.get(name);
  const cls = qual(name, "datasource", opts.naming);
  if (table === undefined) return `${cls} {}`;
  const body = table.fields
    .map((f) => `${opts.naming.fieldName(f.name)}: ${sampleForField(f.type, f.isNullable)}`)
    .join(", ");
  return `${cls} { ${body} }`;
};

const wrapValue = (
  expr: string,
  field: { isArray: boolean; isNullable: boolean },
): string => {
  const inner = field.isArray ? `vec![${expr}]` : expr;
  return field.isNullable ? `Some(${inner})` : inner;
};

const viewFieldTok = (
  field: ViewField,
  opts: ViewTestOpts,
  visited: Set<string>,
): FieldTok => {
  let pair: { sample: string; next: string };
  if (field.kind === "primitive") {
    pair = samplesForNative(convertSpecType(field.base), field.base);
  } else {
    const expr =
      field.kind === "datasource"
        ? renderDs(field.base, opts)
        : viewExpr(field.base, opts, visited);
    pair = { sample: expr, next: expr };
  }
  return {
    ident: opts.naming.fieldName(field.name),
    sampleExpr: wrapValue(pair.sample, field),
    nextExpr: wrapValue(pair.next, field),
    nullable: field.isNullable,
  };
};

export const shapedToks = (
  view: ShapedView,
  opts: ViewTestOpts,
  visited: Set<string>,
): FieldTok[] => {
  const expanded = opts.expandedViews.get(view.name);
  const source =
    expanded?.kind === "shaped" ? expanded : view;
  const fields = source.fields.map((f) => viewFieldTok(f, opts, visited));
  if (
    view.inherits !== null &&
    !isAlias(view) &&
    !inlinesParent(view)
  ) {
    const base = renderDs(view.inherits, opts);
    return [
      { ident: "base", sampleExpr: base, nextExpr: base, nullable: false },
      ...fields,
    ];
  }
  return fields;
};

export const viewExpr = (
  name: string,
  opts: ViewTestOpts,
  visited: Set<string>,
): string => {
  if (visited.has(name)) {
    throw new Error(`cyclic view reference: ${name}`);
  }
  const view = opts.views.get(name);
  if (view === undefined) {
    throw new Error(`unknown view: ${name}`);
  }
  const next = new Set(visited).add(name);
  if (view.kind === "union") {
    const member = view.members[0];
    if (member === undefined) return `${qual(name, "view", opts.naming)} {}`;
    return `${qual(name, "view", opts.naming)}::${opts.naming.className(member)}(${viewExpr(member, opts, next)})`;
  }
  const cls = qual(name, "view", opts.naming);
  const fields = shapedToks(view, opts, next);
  return `${cls} { ${fields.map((f) => `${f.ident}: ${f.sampleExpr}`).join(", ")} }`;
};
