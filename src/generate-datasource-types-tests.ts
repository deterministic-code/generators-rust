import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { datasourcePaths, type ArtifactPaths } from "./common/paths.ts";
import { samplesForNative, wrapOption } from "./common/test-samples.ts";
import {
  DeterministicParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTestTmpl } from "./resources/datasource-types-tests.ts";

type EmitOptions = {
  naming: ArtifactPaths;
  schemaVersion: string;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  naming: datasourcePaths(settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
});

const fieldTokens = (
  field: { name: string; type: string; isNullable: boolean },
  opts: EmitOptions,
) => {
  const ident = opts.naming.fieldName(field.name);
  const native = convertSpecType(field.type);
  const { sample, next } = samplesForNative(native, field.type);
  return {
    ident,
    sampleExpr: wrapOption(sample, field.isNullable),
    nextExpr: wrapOption(next, field.isNullable),
    nullable: field.isNullable,
  };
};

const testPath = (entity: string, naming: ArtifactPaths): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const renderTests = (
  table: DatasourceType,
  opts: EmitOptions,
): GenerateEntry => {
  const fields = table.fields.map((f) => fieldTokens(f, opts));
  return content(
    testPath(table.name, opts.naming),
    fill(typeTestTmpl, {
      schemaVersion: opts.schemaVersion,
      structName: opts.naming.className(table.name),
      fileBase: opts.naming.fileBase(table.name),
      fields,
    }),
  );
};

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const opts = emitOptions(settings);
  return deterministic.expandedDatasourceTypes.map((table) =>
    renderTests(table, opts),
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
