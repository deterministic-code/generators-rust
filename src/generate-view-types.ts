import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
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
import { Emit } from "./emit.ts";

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    const expandedByName = new Map(
      deterministic.expandedViewTypes.map((v) => [v.name, v]),
    );
    return deterministic.viewTypes.map((view) =>
      this.view(view, expandedByName.get(view.name)),
    );
  }

  private rustTypeFor(field: ViewField): string {
    let base =
      field.kind === "primitive"
        ? convertSpecType(field.base)
        : field.kind === "datasource"
          ? this.imports.datasourceQual(field.base)
          : this.imports.viewQual(field.base);
    if (field.isArray) base = `Vec<${base}>`;
    return field.isNullable ? `Option<${base}>` : base;
  }

  private structFields(view: ShapedView, expanded: ViewType | undefined) {
    const fields = emitViewFields(view, expanded).map((f) => ({
      ident: this.casing.convertFields(f.name),
      rustType: this.rustTypeFor(f),
    }));
    if (view.inherits !== null && !isAlias(view) && !inlinesParent(view)) {
      return [
        {
          ident: "base",
          rustType: this.imports.datasourceQual(view.inherits),
        },
        ...fields,
      ];
    }
    return fields;
  }

  private view(
    view: ViewType,
    expanded: ViewType | undefined,
  ): GenerateEntry {
    const structName = this.casing.convertTypes(view.name);
    const isUnion = view.kind === "union";
    const alias = !isUnion && isAlias(view);
    const isStruct = !isUnion && !alias;
    const fields = isUnion ? [] : this.structFields(view, expanded);
    return content(
      this.imports.view(view.name),
      fill(typeTmpl, {
        schemaVersion: this.settings.schemaVersion,
        simpleDoc: this.settings.simpleDoc,
        descriptionDoc: this.settings.descriptionDoc,
        structName,
        datasourceType: isUnion ? "standard" : (view.inherits ?? "standard"),
        target: isUnion ? "UnionView" : "ShapedView",
        fieldCount: String(
          isUnion ? view.members.length : isStruct ? fields.length : 0,
        ),
        isAlias: alias,
        aliasType:
          !isUnion && view.inherits !== null
            ? this.imports.datasourceQual(view.inherits)
            : "",
        isUnion,
        isStruct,
        members: isUnion
          ? view.members.map((m) => ({
              variant: this.casing.convertTypes(m),
              memberType: this.imports.viewQual(m),
            }))
          : [],
        fields,
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(VIEW_TYPES_YAML);
  return new Generator(ctx.settings).from(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  );
};
