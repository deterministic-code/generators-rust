import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { createCasing, type PackCasing } from "./common/default-casing.ts";
import {
  DeterministicParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTmpl } from "./resources/datasource-types.ts";

const docTokens = (settings: Record<string, string>) => {
  const comments = settings["comments"];
  return {
    simpleDoc: comments !== "none" && comments !== "description",
    descriptionDoc: comments === "description",
  };
};

type EmitOptions = {
  casing: PackCasing;
  schemaVersion: string;
  simpleDoc: boolean;
  descriptionDoc: boolean;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  casing: createCasing(settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
  ...docTokens(settings),
});

const rustTypeFor = (field: {
  type: string;
  isNullable: boolean;
}): string => {
  const t = convertSpecType(field.type);
  return field.isNullable ? `Option<${t}>` : t;
};

const renderType = (
  table: DatasourceType,
  opts: EmitOptions,
): GenerateEntry => {
  const { casing, schemaVersion, simpleDoc, descriptionDoc } = opts;
  const fields = table.fields;
  const structName = casing.convertTypes(table.name);
  return content(
    casing.filePath(table.name),
    fill(typeTmpl, {
      schemaVersion,
      simpleDoc,
      descriptionDoc,
      structName,
      datasourceType: table.datasourceType,
      fieldCount: String(fields.length),
      fields: fields.map((f) => ({
        ident: casing.convertFields(f.name),
        rustType: rustTypeFor(f),
      })),
    }),
  );
};

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const opts = emitOptions(settings);
  return deterministic.expandedDatasourceTypes.map((table) =>
    renderType(table, opts),
  );
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
