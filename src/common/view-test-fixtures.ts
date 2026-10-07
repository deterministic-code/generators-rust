import {
  dictionaryEntryFields,
  dictionaryOfField,
  isCollectionField,
} from "@deterministic-code/generators-common/spec-types";
import {
  isAlias,
  isUnionEnum,
  wrapsInheritedDatasource,
} from "./view-shape.ts";
import { sampleForField, samplesForNative } from "./test-samples.ts";
import { convertSpecType } from "../base-type-converter.ts";
import type { PackCasing } from "./default-casing.ts";
import type { RustImportGenerator } from "../import-generator.ts";
import type { Type, TypeField } from "../specification-parser.ts";
import { fieldRefKind } from "./view-shape.ts";

export type ViewTestOpts = {
  casing: PackCasing;
  imports: RustImportGenerator;
  tables: Map<string, Type>;
  views: Map<string, Type>;
  expandedViews: Map<string, Type>;
  typesByName: Map<string, Type>;
  datasourceNames: Set<string>;
};

export type FieldTok = {
  ident: string;
  sampleExpr: string;
  nextExpr: string;
  nullable: boolean;
  getsTest: string;
  setsTest: string;
  allowsNoneTest: string;
};

export const renderDs = (name: string, opts: ViewTestOpts): string => {
  const table = opts.tables.get(name);
  const cls = opts.imports.datasourceQual(name);
  if (table === undefined) return `${cls} {}`;
  if (table.fields.length === 0) return `${cls} {}`;
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
  field: { isArray: boolean; isMap?: boolean; isNullable: boolean },
): string => {
  const inner = field.isMap === true
    ? `HashMap::from([(String::from("k"), ${expr})])`
    : field.isArray
      ? `vec![${expr}]`
      : expr;
  return field.isNullable ? `Some(${inner})` : inner;
};

const viewFieldTok = (
  field: TypeField,
  opts: ViewTestOpts,
  visited: Set<string>,
): FieldTok => {
  let pair: { sample: string; next: string };
  const dict = dictionaryOfField(field, opts.typesByName);
  const entry = dict === undefined ? undefined : dictionaryEntryFields(dict);
  const target = entry?.value ?? field;
  const refKind = fieldRefKind(target, opts.typesByName);
  if (refKind === "primitive") {
    pair = samplesForNative(convertSpecType(target.base), target.base);
  } else {
    const expr =
      refKind === "datasource"
        ? renderDs(target.base, opts)
        : viewExpr(target.base, opts, visited);
    pair = { sample: expr, next: expr };
  }
  return {
    ident: opts.casing.convertFields(field.name),
    sampleExpr: wrapValue(pair.sample, field),
    nextExpr: wrapValue(pair.next, field),
    nullable: field.isNullable,
    getsTest: opts.casing.fnIdent(`gets_${field.name}`),
    setsTest: opts.casing.fnIdent(`sets_${field.name}`),
    allowsNoneTest: opts.casing.fnIdent(
      `allows_setting_${field.name}_to_none`,
    ),
  };
};

const baseTok = (
  ident: string,
  expr: string,
  opts: ViewTestOpts,
): FieldTok => ({
  ident,
  sampleExpr: expr,
  nextExpr: expr,
  nullable: false,
  getsTest: opts.casing.fnIdent(`gets_${ident}`),
  setsTest: opts.casing.fnIdent(`sets_${ident}`),
  allowsNoneTest: opts.casing.fnIdent(`allows_setting_${ident}_to_none`),
});

export const shapedToks = (
  view: Type,
  opts: ViewTestOpts,
  visited: Set<string>,
): FieldTok[] => {
  const expanded = opts.expandedViews.get(view.name);
  if (isAlias(view)) {
    const extras = (expanded ?? view).fields
      .filter(isCollectionField)
      .map((f) => viewFieldTok(f, opts, visited));
    if (extras.length === 0) {
      return (expanded ?? view).fields.map((f) => viewFieldTok(f, opts, visited));
    }
    return [baseTok("base", renderDs(view.name, opts), opts), ...extras];
  }
  const source = wrapsInheritedDatasource(view, opts.datasourceNames)
    ? view
    : (expanded ?? view);
  const fields = source.fields.map((f) => viewFieldTok(f, opts, visited));
  if (wrapsInheritedDatasource(view, opts.datasourceNames) && view.inherits) {
    return [baseTok("base", renderDs(view.inherits, opts), opts), ...fields];
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
  const cls = opts.imports.viewQual(name);
  if (isUnionEnum(view)) {
    const first = view.union![0]!;
    return `${cls}::${opts.casing.convertTypes(first)}(${viewExpr(first, opts, next)})`;
  }
  if (isAlias(view)) {
    const extras = (opts.expandedViews.get(view.name) ?? view).fields.filter(
      isCollectionField,
    );
    if (extras.length === 0) return renderDs(view.name, opts);
  }
  const fields = shapedToks(view, opts, next);
  if (fields.length === 0) return `${cls} {}`;
  return `${cls} { ${fields.map((f) => `${f.ident}: ${f.sampleExpr}`).join(", ")} }`;
};
