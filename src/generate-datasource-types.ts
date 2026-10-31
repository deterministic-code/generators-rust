import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  datasourceTypesOf,
  fieldTypeOf,
  persistedColumnFields,
  tableKind,
} from "@deterministic-code/generators-common/spec-types";
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
import { typesNeedingDefault } from "./struct-default.ts";
import { typeTmpl } from "./resources/datasource-types.ts";
import { Emit } from "./emit.ts";

class Generator extends Emit {
  private qualify(path: string): string {
    return qualifyRustPath(this.raw, path);
  }

  private rustTypeFor(
    field: TypeField,
    typesByName: ReadonlyMap<string, Type>,
  ): { rustType: string; native: string } {
    const native = convertSpecType(fieldTypeOf(field, typesByName));
    const t = this.qualify(native);
    return {
      rustType: field.isNullable ? `Option<${t}>` : t,
      native: field.isNullable ? `Option<${native}>` : native,
    };
  }

  from(deterministic: IDeterministic): GenerateEntry[] {
    const typesByName = new Map(
      deterministic.expandedTypes.map((t) => [t.name, t]),
    );
    const needingDefault = typesNeedingDefault(deterministic);
    return datasourceTypesOf(deterministic).map((table) =>
      this.type(table, typesByName, needingDefault),
    );
  }

  private type(
    table: Type,
    typesByName: Map<string, Type>,
    needingDefault: Set<string>,
  ): GenerateEntry {
    const implDefault = needingDefault.has(table.name);
    const facet = this.raw["backend.datasources"] !== undefined && this.raw["backend.datasources"] !== "";
    const fields = persistedColumnFields(table, typesByName).map((f) => {
      const { rustType, native } = this.rustTypeFor(f, typesByName);
      const specExpr = rustSpecDefaultExpr(fieldTypeOf(f, typesByName), f);
      return {
        ident: this.casing.convertFields(f.name),
        rustType,
        defaultExpr: this.qualify(specExpr ?? rustLanguageZero(native)),
      };
    });
    const structName = this.casing.convertTypes(table.name);
    return content(
      this.imports.datasource(table.name),
      fill(typeTmpl, {
        schemaVersion: this.settings.schemaVersion,
        simpleDoc: this.settings.simpleDoc,
        descriptionDoc: this.settings.descriptionDoc,
        structName,
        datasourceType: tableKind(table),
        fieldCount: String(fields.length),
        implDefault,
        deriveDefault: facet && !implDefault,
        facet,
        fields,
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(TYPES_YAML);
  return new Generator(ctx.settings).from(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  );
};
