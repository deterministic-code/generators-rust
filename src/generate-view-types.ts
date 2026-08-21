import { pascalCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  createImportGenerator,
  type RustImportGenerator,
} from "./import-generator.ts";
import {
  emitViewFields,
  inlinesParent,
  isAlias,
} from "./common/view-shape.ts";
import {
  DeterministicParser,
  VIEW_TYPES_YAML,
  type ShapedView,
  type ViewField,
  type ViewType,
  type IDeterministic,
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
  imports: RustImportGenerator;
  schemaVersion: string;
  simpleDoc: boolean;
  descriptionDoc: boolean;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  imports: createImportGenerator(".", settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
  ...docTokens(settings),
});

const className = (entity: string): string => pascalCase(entity);
const fieldName = (field: string): string => field;

const rustTypeFor = (field: ViewField, opts: EmitOptions): string => {
  let base =
    field.kind === "primitive"
      ? convertSpecType(field.base)
      : field.kind === "datasource"
        ? opts.imports.datasourceQual(field.base)
        : opts.imports.viewQual(field.base);
  if (field.isArray) base = `Vec<${base}>`;
  return field.isNullable ? `Option<${base}>` : base;
};

const structFields = (
  view: ShapedView,
  expanded: ViewType | undefined,
  opts: EmitOptions,
) => {
  const fields = emitViewFields(view, expanded).map((f) => ({
    ident: fieldName(f.name),
    rustType: rustTypeFor(f, opts),
  }));
  if (
    view.inherits !== null &&
    !isAlias(view) &&
    !inlinesParent(view)
  ) {
    return [
      {
        ident: "base",
        rustType: opts.imports.datasourceQual(view.inherits),
      },
      ...fields,
    ];
  }
  return fields;
};

const renderView = (
  view: ViewType,
  expanded: ViewType | undefined,
  opts: EmitOptions,
): GenerateEntry => {
  const structName = className(view.name);
  const isUnion = view.kind === "union";
  const alias = !isUnion && isAlias(view);
  const isStruct = !isUnion && !alias;
  const fields = isUnion ? [] : structFields(view, expanded, opts);
  return content(
    opts.imports.view(view.name),
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
          ? opts.imports.datasourceQual(view.inherits)
          : "",
      isUnion,
      isStruct,
      members: isUnion
        ? view.members.map((m) => ({
            variant: className(m),
            memberType: opts.imports.viewQual(m),
          }))
        : [],
      fields,
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
