import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { viewPaths, type ArtifactPaths } from "./common/paths.ts";
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
  naming: ArtifactPaths;
  schemaVersion: string;
  simpleDoc: boolean;
  descriptionDoc: boolean;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
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

const rustTypeFor = (field: ViewField, opts: EmitOptions): string => {
  let base =
    field.kind === "primitive"
      ? convertSpecType(field.base)
      : qual(field.base, field.kind, opts.naming);
  if (field.isArray) base = `Vec<${base}>`;
  return field.isNullable ? `Option<${base}>` : base;
};

const structFields = (
  view: ShapedView,
  expanded: ViewType | undefined,
  opts: EmitOptions,
) => {
  const fields = emitViewFields(view, expanded).map((f) => ({
    ident: opts.naming.fieldName(f.name),
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
        rustType: qual(view.inherits, "datasource", opts.naming),
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
  const structName = opts.naming.className(view.name);
  const isUnion = view.kind === "union";
  const alias = !isUnion && isAlias(view);
  const isStruct = !isUnion && !alias;
  const fields = isUnion ? [] : structFields(view, expanded, opts);
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
