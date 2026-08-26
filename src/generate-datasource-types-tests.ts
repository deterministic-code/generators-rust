import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  columnFields,
  datasourceTypesOf,
} from "@deterministic-code/generators-common/spec-types";
import { samplesForNative, wrapOption } from "./common/test-samples.ts";
import {
  DeterministicParser,
  TYPES_YAML,
  type IDeterministic,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTestTmpl } from "./resources/datasource-types-tests.ts";
import { Emit } from "./emit.ts";

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    return datasourceTypesOf(deterministic).map((table) => {
      const fields = columnFields(table.fields).map((field) => {
        const ident = this.casing.convertFields(field.name);
        const native = convertSpecType(field.type);
        const { sample, next } = samplesForNative(native, field.type);
        return {
          ident,
          sampleExpr: wrapOption(sample, field.isNullable),
          nextExpr: wrapOption(next, field.isNullable),
          nullable: field.isNullable,
          getsTest: this.casing.fnIdent(`gets_${field.name}`),
          setsTest: this.casing.fnIdent(`sets_${field.name}`),
          allowsNoneTest: this.casing.fnIdent(
            `allows_setting_${field.name}_to_none`,
          ),
        };
      });
      const src = this.imports.datasource(table.name);
      return content(
        this.imports.test(src, table.name),
        fill(typeTestTmpl, {
          schemaVersion: this.settings.schemaVersion,
          structName: this.casing.convertTypes(table.name),
          fileBase: this.casing.fileBase(table.name),
          fields,
        }),
      );
    });
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
