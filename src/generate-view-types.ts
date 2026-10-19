import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  authoredViewTypesOf,
  datasourceTypesOf,
  dictionaryEntryFields,
  dictionaryOfField,
  fieldTypeOf,
  tableKind,
  viewTypesOf,
} from "@deterministic-code/generators-common/spec-types";
import {
  emitViewFields,
  fieldRefKind,
  isAlias,
  isUnionEnum,
  wrapsInheritedDatasource,
} from "./common/view-shape.ts";
import {
  DeterministicParser,
  TYPES_YAML,
  type IDeterministic,
  type Type,
  type TypeField,
} from "./specification-parser.ts";
import {
  convertSpecType,
  rustLanguageZero,
  rustSpecDefaultExpr,
} from "./deterministic-type-converter.ts";
import { typeTmpl } from "./resources/view-types.ts";
import { Emit } from "./emit.ts";
import { typesNeedingDefault } from "./struct-default.ts";

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
    const needingDefault = typesNeedingDefault(deterministic);
    return authoredViewTypesOf(deterministic).map((view) =>
      this.view(view, this.expandedByName.get(view.name), needingDefault),
    );
  }

  private rustPart(field: TypeField): string {
    const refKind = fieldRefKind(field, this.typesByName);
    return refKind === "primitive"
      ? convertSpecType(field.base)
      : refKind === "datasource"
        ? this.imports.datasourceQual(field.base)
        : this.imports.viewQual(field.base);
  }

  private rustTypeFor(field: TypeField): string {
    const dict = dictionaryOfField(field, this.typesByName);
    const entry = dict === undefined ? undefined : dictionaryEntryFields(dict);
    let base: string;
    if (entry !== undefined) {
      base = `HashMap<${this.rustPart(entry.key)}, ${this.rustPart(entry.value)}>`;
    } else {
      base = this.rustPart(field);
      if (field.isArray) base = `Vec<${base}>`;
    }
    return field.isNullable ? `Option<${base}>` : base;
  }

  private fieldToken(field: TypeField) {
    const rustType = this.rustTypeFor(field);
    const specExpr =
      field.isArray || field.isMap === true
        ? undefined
        : field.kind === "primitive"
          ? rustSpecDefaultExpr(fieldTypeOf(field, this.typesByName), field)
          : undefined;
    return {
      ident: this.casing.convertFields(field.name),
      rustType,
      defaultExpr: specExpr ?? rustLanguageZero(rustType),
    };
  }

  private structFields(view: Type, expanded: Type | undefined) {
    const fields = emitViewFields(view, expanded, this.datasourceNames).map(
      (f) => this.fieldToken(f),
    );
    const synthetic = (ident: string, rustType: string) => ({
      ident,
      rustType,
      defaultExpr: rustLanguageZero(rustType),
    });
    if (isAlias(view) && fields.length > 0) {
      return [
        synthetic("base", this.imports.datasourceQual(view.name)),
        ...fields,
      ];
    }
    if (
      view.inherits !== undefined &&
      wrapsInheritedDatasource(view, this.datasourceNames)
    ) {
      return [
        synthetic("base", this.imports.datasourceQual(view.inherits)),
        ...fields,
      ];
    }
    return fields;
  }

  private view(
    view: Type,
    expanded: Type | undefined,
    needingDefault: Set<string>,
  ): GenerateEntry {
    const structName = this.casing.convertTypes(view.name);
    const isUnion = isUnionEnum(view);
    const members = (view.union ?? []).map((name) => ({
      variant: this.casing.convertTypes(name),
      memberType: this.imports.viewQual(name),
    }));
    const fields = isUnion ? [] : this.structFields(view, expanded);
    const alias = !isUnion && isAlias(view) && fields.length === 0;
    const isStruct = !alias && !isUnion;
    return content(
      this.imports.view(view.name),
      fill(typeTmpl, {
        schemaVersion: this.settings.schemaVersion,
        simpleDoc: this.settings.simpleDoc,
        descriptionDoc: this.settings.descriptionDoc,
        structName,
        datasourceType: tableKind(view),
        target: isUnion ? "UnionView" : "ShapedView",
        fieldCount: String(isStruct ? fields.length : 0),
        isAlias: alias,
        aliasType: isAlias(view) ? this.imports.datasourceQual(view.name) : "",
        isUnion,
        isStruct,
        implDefault: isStruct && needingDefault.has(view.name),
        needsHashMap: fields.some((f) => f.rustType.includes("HashMap<")),
        members,
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
