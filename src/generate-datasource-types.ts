import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  DeterministicParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTmpl } from "./resources/datasource-types.ts";
import { Emit } from "./emit.ts";

const rustTypeFor = (field: {
  type: string;
  isNullable: boolean;
}): string => {
  const t = convertSpecType(field.type);
  return field.isNullable ? `Option<${t}>` : t;
};

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    return deterministic.expandedDatasourceTypes.map((table) =>
      this.type(table),
    );
  }

  private type(table: DatasourceType): GenerateEntry {
    const fields = table.fields;
    const structName = this.casing.convertTypes(table.name);
    return content(
      this.imports.datasource(table.name),
      fill(typeTmpl, {
        schemaVersion: this.settings.schemaVersion,
        simpleDoc: this.settings.simpleDoc,
        descriptionDoc: this.settings.descriptionDoc,
        structName,
        datasourceType: table.datasourceType,
        fieldCount: String(fields.length),
        fields: fields.map((f) => ({
          ident: this.casing.convertFields(f.name),
          rustType: rustTypeFor(f),
        })),
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(DATASOURCE_TYPES_YAML);
  return new Generator(ctx.settings).from(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  );
};
