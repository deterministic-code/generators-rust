import type { SettingsDict } from "./generate-context.ts";
import { settingsBool, settingsStr } from "./settings.ts";
import { convertSpecType, idTypeToNative } from "./type-converter.ts";

const REFERENCE_SHAPE: Record<string, { type: string; size: number | undefined }> =
  {
    integer: { type: "number", size: undefined },
    biginteger: { type: "biginteger", size: undefined },
    uuid: { type: "uuid", size: undefined },
    string: { type: "string", size: 64 },
  };

export type DatasourceSettings = {
  idType: string;
  datetimeRepr: string;
  withUuidColumn: boolean;
  rustIdType: string;
  useOptimisticConcurrency: boolean;
};

export const datasourceSettings = (
  settings: SettingsDict,
): DatasourceSettings => {
  const idType = settingsStr(settings, "datasource.id_type") ?? "integer";
  return {
    idType,
    datetimeRepr: settingsStr(settings, "datasource.datetime") ?? "native",
    withUuidColumn: idType !== "uuid",
    rustIdType: idTypeToNative(idType),
    useOptimisticConcurrency: settingsBool(
      settings,
      "datasource.use_optimistic_concurrency",
    ),
  };
};

export const referenceFieldShape = (
  idType: string,
): { type: string; size: number | undefined } =>
  REFERENCE_SHAPE[idType] ?? REFERENCE_SHAPE.integer;

export const nativeFieldType = (
  ds: DatasourceSettings,
  field: { name?: string; type: string },
): string =>
  field.name === "id"
    ? ds.rustIdType
    : convertSpecType(field.type, ds.datetimeRepr);

export type SystemColumn = {
  name: string;
  type: string;
  isNullable: boolean;
};

export const systemColumns = (ds: DatasourceSettings): SystemColumn[] => [
  { name: "id", type: referenceFieldShape(ds.idType).type, isNullable: false },
  ...(ds.withUuidColumn
    ? [{ name: "uuid", type: "uuid", isNullable: false }]
    : []),
  { name: "created", type: "datetime", isNullable: false },
  { name: "updated", type: "datetime", isNullable: false },
];

export const declaredFields = <T extends { name: string }>(
  fields: T[],
  ds: DatasourceSettings,
): T[] =>
  ds.withUuidColumn ? fields : fields.filter((f) => f.name !== "uuid");

export const tableFields = <T extends { name: string }>(
  fields: T[],
  ds: DatasourceSettings,
): Array<T | SystemColumn> => {
  const injected = systemColumns(ds);
  const seen = new Set(injected.map((f) => f.name));
  return [
    ...injected,
    ...declaredFields(fields, ds).filter((f) => !seen.has(f.name)),
  ];
};
