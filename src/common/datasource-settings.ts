import type { SettingsDict } from "./generate-context.ts";
import { settingsStr } from "./settings.ts";

const ID_RUST: Record<string, string> = {
  integer: "i64",
  biginteger: "i64",
  uuid: "uuid::Uuid",
  string: "String",
};

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
};

export const datasourceSettings = (
  settings: SettingsDict,
): DatasourceSettings => {
  const idType = settingsStr(settings, "datasource.id_type") ?? "integer";
  return {
    idType,
    datetimeRepr: settingsStr(settings, "datasource.datetime") ?? "native",
    withUuidColumn: idType !== "uuid",
    rustIdType: idType === "i32" ? "i32" : (ID_RUST[idType] ?? "i64"),
  };
};

export const referenceFieldShape = (
  idType: string,
): { type: string; size: number | undefined } =>
  REFERENCE_SHAPE[idType] ?? REFERENCE_SHAPE.integer;
