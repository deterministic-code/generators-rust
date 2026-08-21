import { pascalCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { createImportGenerator } from "./import-generator.ts";
import { samplesForNative, wrapOption } from "./common/test-samples.ts";
import {
  DeterministicParser,
  DATASOURCE_TYPES_YAML,
  type IDeterministic,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTestTmpl } from "./resources/datasource-types-tests.ts";

const className = (entity: string): string => pascalCase(entity);
const fieldName = (field: string): string => field;

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const imports = createImportGenerator(".", settings);
  const schemaVersion = settings["codegen.schema_version"] ?? "1.0";
  return deterministic.expandedDatasourceTypes.map((table) => {
    const fields = table.fields.map((field) => {
      const ident = fieldName(field.name);
      const native = convertSpecType(field.type);
      const { sample, next } = samplesForNative(native, field.type);
      return {
        ident,
        sampleExpr: wrapOption(sample, field.isNullable),
        nextExpr: wrapOption(next, field.isNullable),
        nullable: field.isNullable,
      };
    });
    const src = imports.datasource(table.name);
    return content(
      imports.test(src, table.name),
      fill(typeTestTmpl, {
        schemaVersion,
        structName: className(table.name),
        fileBase: table.name,
        fields,
      }),
    );
  });
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(DATASOURCE_TYPES_YAML);
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
    ctx.settings,
  );
};
