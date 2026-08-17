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

export const convertSpecType = (
  specType: string,
  datetimeRepr: string,
): string => {
  if (specType === "datetime" && datetimeRepr === "string") return "String";
  const native = NATIVE[specType];
  if (!native) throw new Error(`Unknown spec field type: ${specType}`);
  return native;
};
