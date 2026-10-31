import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  authoredViewTypesOf,
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
  qualifyRustPath,
  rustLanguageZero,
  rustSpecDefaultExpr,
} from "./deterministic-type-converter.ts";
import { typeTmpl } from "./resources/view-types.ts";
import { Emit } from "./emit.ts";
import { typesNeedingDefault } from "./struct-default.ts";

class Generator extends Emit {
  private readonly typesByName: Map<string, Type>;
  private readonly expandedByName: Map<string, Type>;

  constructor(raw: Record<string, string>, deterministic: IDeterministic) {
    super(raw);
    this.typesByName = new Map(
      deterministic.expandedTypes.map((t) => [t.name, t]),
    );
    this.expandedByName = new Map(
      viewTypesOf(deterministic).map((v) => [v.name, v]),
    );
  }

  from(deterministic: IDeterministic): GenerateEntry[] {
    const needingDefault = typesNeedingDefault(deterministic);
    return authoredViewTypesOf(deterministic).flatMap((view) => {
      const entry = this.view(view, this.expandedByName.get(view.name), needingDefault);
      return entry === undefined ? [] : [entry];
    });
  }

  private qualify(path: string): string {
    return qualifyRustPath(this.raw, path);
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
      rustType: this.qualify(rustType),
      defaultExpr: this.qualify(specExpr ?? rustLanguageZero(rustType)),
    };
  }

  private structFields(view: Type, expanded: Type | undefined) {
    return emitViewFields(view, expanded).map((f) => this.fieldToken(f));
  }

  private view(
    view: Type,
    expanded: Type | undefined,
    needingDefault: Set<string>,
  ): GenerateEntry | undefined {
    const structName = this.casing.convertTypes(view.name);
    const isUnion = isUnionEnum(view);
    const members = (view.union ?? []).map((name) => ({
      variant: this.casing.convertTypes(name),
      memberType: this.imports.viewQual(name),
    }));
    const fields = isUnion ? [] : this.structFields(view, expanded);
    const alias = !isUnion && isAlias(view) && fields.length === 0;
    const aliasType = isAlias(view) ? this.imports.datasourceQual(view.name) : "";
    if (alias && aliasType === structName) return undefined;
    const isStruct = !alias && !isUnion;
    const implDefault = isStruct && needingDefault.has(view.name);
    const facet = isStruct && this.raw["backend.datasources"] !== undefined && this.raw["backend.datasources"] !== "";
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
        aliasType,
        isUnion,
        isStruct,
        implDefault,
        deriveDefault: facet && !implDefault,
        facet,
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
