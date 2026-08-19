/** Map a `backend/types.yaml` field type to a Rust type. */
const NATIVE: Record<string, string> = {
  string: "String",
  character: "String",
  number: "i64",
  integer: "i32",
  unsignedinteger: "u32",
  smallinteger: "i16",
  unsignedsmallinteger: "u16",
  biginteger: "i64",
  unsignedbiginteger: "u64",
  float: "f64",
  decimal: "String",
  boolean: "bool",
  datetime: "chrono::DateTime<chrono::Utc>",
  binary: "Vec<u8>",
  uuid: "String",
  reference: "i64",
};

/** `datasource.id_type` → Rust id type. */
const ID_NATIVE: Record<string, string> = {
  integer: "i64",
  biginteger: "i64",
  uuid: "uuid::Uuid",
  string: "String",
  i32: "i32",
};

/** Spec integer-like type → literal suffix used in validator comparisons. */
const INT_LITERAL_SUFFIX: Record<string, string> = {
  number: "i64",
  biginteger: "i64",
  reference: "i64",
  integer: "i32",
  smallinteger: "i16",
};

/** `datasource.id_type` → literal suffix for id comparisons. */
const ID_LITERAL_SUFFIX: Record<string, string> = {
  i32: "i32",
  i64: "i64",
  integer: "i64",
  biginteger: "i64",
};

export const convertSpecType = (
  specType: string,
  datetimeRepr: string,
): string => {
  if (specType === "datetime" && datetimeRepr === "string") return "String";
  const native = NATIVE[specType];
  if (!native) throw new Error(`Unknown spec field type: ${specType}`);
  return native;
};

export const idTypeToNative = (idType: string): string =>
  ID_NATIVE[idType] ?? ID_NATIVE.integer;

export const nativeFieldType = (
  ds: { idType: string; datetimeRepr: string },
  field: { name?: string; type: string; references?: string },
): string =>
  field.name === "id" || field.references?.split(".")[1] === "id"
    ? idTypeToNative(ds.idType)
    : convertSpecType(field.type, ds.datetimeRepr);

export const intLiteralSuffix = (specType: string): string | undefined =>
  INT_LITERAL_SUFFIX[specType];

export const idTypeLiteralSuffix = (idType: string): string | undefined =>
  ID_LITERAL_SUFFIX[idType];
