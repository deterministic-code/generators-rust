import { inlinesParent, isAlias } from "./view-shape.ts";
import { sampleForField, samplesForNative } from "./test-samples.ts";
import { convertSpecType } from "../base-type-converter.ts";
import type { PackCasing } from "./default-casing.ts";
import type { RustImportGenerator } from "../import-generator.ts";
import type {
  DatasourceType,
  ShapedView,
  ViewField,
  ViewType,
} from "../specification-parser.ts";

export type ViewTestOpts = {
  casing: PackCasing;
  imports: RustImportGenerator;
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

export const renderDs = (name: string, opts: ViewTestOpts): string => {
  const table = opts.tables.get(name);
  const cls = opts.imports.datasourceQual(name);
  if (table === undefined) return `${cls} {}`;
  const body = table.fields
    .map(
      (f) =>
        `${opts.casing.convertFields(f.name)}: ${sampleForField(f.type, f.isNullable)}`,
    )
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
    ident: opts.casing.convertFields(field.name),
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
    if (member === undefined) return `${opts.imports.viewQual(name)} {}`;
    return `${opts.imports.viewQual(name)}::${opts.casing.convertTypes(member)}(${viewExpr(member, opts, next)})`;
  }
  const cls = opts.imports.viewQual(name);
  const fields = shapedToks(view, opts, next);
  return `${cls} { ${fields.map((f) => `${f.ident}: ${f.sampleExpr}`).join(", ")} }`;
};
