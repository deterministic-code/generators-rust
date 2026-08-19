import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { datasourcePaths, type ArtifactPaths } from "./common/paths.ts";
import {
  SpecificationParser,
  type DatasourceType,
} from "./specification-parser.ts";
import { nativeFieldType } from "./common/type-converter.ts";
import { typeTmpl } from "./resources/datasource-types.ts";
import { systemColumnsInjectedFor } from "./system-columns.ts";

type Datasource = {
  idType: string;
  datetimeRepr: string;
  withUuidColumn: boolean;
  useOptimisticConcurrency: boolean;
};

const datasource = (settings: Record<string, string>): Datasource => {
  const idType = settings["datasource.id_type"] ?? "integer";
  return {
    idType,
    datetimeRepr: settings["datasource.datetime"] ?? "native",
    withUuidColumn: idType !== "uuid",
    useOptimisticConcurrency:
      settings["datasource.use_optimistic_concurrency"] === "true",
  };
};

const SYSTEM = {
  id: { type: "number", isNullable: false },
  uuid: { type: "uuid", isNullable: false },
  created: { type: "datetime", isNullable: false },
  updated: { type: "datetime", isNullable: false },
} as const;

const docTokens = (settings: Record<string, string>) => {
  const comments = settings["comments"];
  return {
    simpleDoc: comments !== "none" && comments !== "description",
    descriptionDoc: comments === "description",
  };
};

type EmitOptions = {
  ds: Datasource;
  naming: ArtifactPaths;
  schemaVersion: string;
  simpleDoc: boolean;
  descriptionDoc: boolean;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => {
  const ds = datasource(settings);
  return {
    ds,
    naming: datasourcePaths(settings),
    schemaVersion: settings["codegen.schema_version"] ?? "1.0",
    ...docTokens(settings),
  };
};

const tableFields = (
  table: DatasourceType,
  ds: Datasource,
): Array<{ name: string; type: string; isNullable: boolean }> => {
  const injected = systemColumnsInjectedFor({
    datasource_type: table.datasourceType,
    fields: table.fields.map((f) => ({
      [f.name]: f.isPrimaryKey ? { primary_key: true } : {},
    })),
  });
  const declared = new Set(table.fields.map((f) => f.name));
  const system = (["id", "uuid", "created", "updated"] as const)
    .filter(
      (n) =>
        injected.has(n) &&
        !declared.has(n) &&
        (n !== "uuid" || ds.withUuidColumn),
    )
    .map((name) => ({ name, ...SYSTEM[name] }));
  return [...system, ...table.fields].filter(
    (f) => ds.withUuidColumn || f.name !== "uuid",
  );
};

const rustTypeFor = (
  field: {
    name: string;
    type: string;
    isNullable: boolean;
    references?: string;
  },
  ds: Datasource,
): string => {
  const t = nativeFieldType(ds, field);
  return field.isNullable ? `Option<${t}>` : t;
};

const renderType = (
  table: DatasourceType,
  opts: EmitOptions,
): GenerateEntry => {
  const { ds, naming, schemaVersion, simpleDoc, descriptionDoc } = opts;
  const fields = tableFields(table, ds);
  const structName = naming.className(table.name);
  return content(
    naming.filePath(table.name),
    fill(typeTmpl, {
      schemaVersion,
      simpleDoc,
      descriptionDoc,
      structName,
      datasourceType: table.datasourceType,
      fieldCount: String(fields.length),
      fields: fields.map((f) => ({
        ident: naming.fieldName(f.name),
        rustType: rustTypeFor(f, ds),
      })),
    }),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const opts = emitOptions(ctx.settings);
  const types = await new SpecificationParser(ctx.reader).loadDatasourceTypes(opts.ds.idType);
  return types.map((table) => renderType(table, opts));
};
