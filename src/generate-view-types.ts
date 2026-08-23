import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  authoredViewTypesOf,
  datasourceTypesOf,
  tableKind,
  unionMembers,
  viewTypesOf,
} from "@deterministic-code/generators-common/spec-types";
import {
  emitViewFields,
  fieldRefKind,
  isAlias,
  wrapsInheritedDatasource,
} from "./common/view-shape.ts";
import {
  DeterministicParser,
  TYPES_YAML,
  type IDeterministic,
  type Type,
  type TypeField,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTmpl } from "./resources/view-types.ts";
import { Emit } from "./emit.ts";

class Generator extends Emit {
  private readonly typesByName: Map<string, Type>;
  private readonly datasourceNames: Set<string>;
  private readonly expandedByName: Map<string, Type>;

  constructor(raw: Record<string, string>, deterministic: IDeterministic) {
    super(raw);
    this.typesByName = new Map(
      deterministic.expandedTypes.map((t) => [t.name, t]),
    );
    this.datasourceNames = new Set(
      datasourceTypesOf(deterministic).map((t) => t.name),
    );
    this.expandedByName = new Map(
      viewTypesOf(deterministic).map((v) => [v.name, v]),
    );
  }

  from(deterministic: IDeterministic): GenerateEntry[] {
    return authoredViewTypesOf(deterministic).map((view) =>
      this.view(view, this.expandedByName.get(view.name)),
    );
  }

  private rustTypeFor(field: TypeField): string {
    const refKind = fieldRefKind(field, this.typesByName);
    let base =
      refKind === "primitive"
        ? convertSpecType(field.base)
        : refKind === "datasource"
          ? this.imports.datasourceQual(field.base)
          : this.imports.viewQual(field.base);
    if (field.isArray) base = `Vec<${base}>`;
    return field.isNullable ? `Option<${base}>` : base;
  }

  private structFields(view: Type, expanded: Type | undefined) {
    const fields = emitViewFields(view, expanded, this.datasourceNames).map(
      (f) => ({
        ident: this.casing.convertFields(f.name),
        rustType: this.rustTypeFor(f),
      }),
    );
    if (
      view.inherits !== undefined &&
      wrapsInheritedDatasource(view, this.datasourceNames)
    ) {
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

  private view(view: Type, expanded: Type | undefined): GenerateEntry {
    const structName = this.casing.convertTypes(view.name);
    const members = unionMembers(view);
    const isUnion = members !== undefined;
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
        datasourceType: isUnion
          ? "standard"
          : tableKind(view),
        target: isUnion ? "UnionView" : "ShapedView",
        fieldCount: String(
          isUnion ? members.length : isStruct ? fields.length : 0,
        ),
        isAlias: alias,
        aliasType:
          !isUnion && isAlias(view)
            ? this.imports.datasourceQual(view.name)
            : "",
        isUnion,
        isStruct,
        members: isUnion
          ? members.map((m) => ({
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
  await ctx.reader.read(TYPES_YAML);
  const deterministic = await DeterministicParser(ctx.reader).parse(
    ctx.settings,
  );
  return new Generator(ctx.settings, deterministic).from(deterministic);
};
