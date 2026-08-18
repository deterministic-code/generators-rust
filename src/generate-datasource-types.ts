import {
  datasourceSettings,
  type DatasourceSettings,
} from "./common/datasource-settings.ts";
import { commentStyle, type CommentStyle } from "./common/doc-comment.ts";
import { fill } from "./common/fill.ts";
import type { GenerateContext, SettingsDict } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import { rustNaming, type ArtifactNaming } from "./common/naming.ts";
import {
  loadDatasourceTypes,
  type DatasourceType,
} from "./common/parse-datasource-types.ts";
import { settingsStr } from "./common/settings.ts";
import { convertSpecType } from "./common/type-converter.ts";
import { typeTmpl } from "./resources/datasource-types.ts";
import { systemColumnsInjectedFor } from "./system-columns.ts";

const SYSTEM = {
  id: { type: "number", isNullable: false },
  uuid: { type: "uuid", isNullable: false },
  created: { type: "datetime", isNullable: false },
  updated: { type: "datetime", isNullable: false },
} as const;

type EmitOptions = {
  ds: DatasourceSettings;
  naming: ArtifactNaming;
  schemaVersion: string;
  style: CommentStyle;
};

const emitOptions = (settings: SettingsDict): EmitOptions => {
  const ds = datasourceSettings(settings);
  return {
    ds,
    naming: rustNaming(settings),
    schemaVersion: settingsStr(settings, "codegen.schema_version") ?? "1.0",
    style: commentStyle(settingsStr(settings, "comments")),
  };
};

const tableFields = (
  table: DatasourceType,
  ds: DatasourceSettings,
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
  field: { name: string; type: string; isNullable: boolean },
  ds: DatasourceSettings,
): string => {
  const t =
    field.name === "id"
      ? ds.rustIdType
      : convertSpecType(field.type, ds.datetimeRepr);
  return field.isNullable ? `Option<${t}>` : t;
};

const renderType = (
  table: DatasourceType,
  opts: EmitOptions,
): GenerateEntry => {
  const { ds, naming, schemaVersion, style } = opts;
  const fields = tableFields(table, ds);
  const structName = naming.className(table.name);
  return content(
    naming.filePath(table.name),
    fill(typeTmpl, {
      schemaVersion,
      simpleDoc: style === "simple",
      descriptionDoc: style === "description",
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
  const types = await loadDatasourceTypes(ctx.reader, opts.ds.idType);
  return types.map((table) => renderType(table, opts));
};
