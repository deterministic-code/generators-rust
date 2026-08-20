import { convertSpecType } from "../base-type-converter.ts";

export const rustString = (value: string): string =>
  `String::from(${JSON.stringify(value)})`;

const rustDatetime = (iso: string): string =>
  `chrono::DateTime::parse_from_rfc3339(${JSON.stringify(iso)}).unwrap().with_timezone(&chrono::Utc)`;

export const samplesForNative = (
  native: string,
  fieldType: string,
): { sample: string; next: string } => {
  switch (native) {
    case "i64":
      return { sample: "1i64", next: "2i64" };
    case "i32":
      return { sample: "1i32", next: "2i32" };
    case "i16":
      return { sample: "1i16", next: "2i16" };
    case "u64":
      return { sample: "1u64", next: "2u64" };
    case "u32":
      return { sample: "1u32", next: "2u32" };
    case "u16":
      return { sample: "1u16", next: "2u16" };
    case "f64":
      return { sample: "1.0f64", next: "2.0f64" };
    case "bool":
      return { sample: "false", next: "true" };
    case "uuid::Uuid":
      return {
        sample: "uuid::Uuid::nil()",
        next: "uuid::Uuid::from_u128(1)",
      };
    case "Vec<u8>":
      return { sample: "Vec::<u8>::new()", next: "vec![1u8]" };
    case "chrono::DateTime<chrono::Utc>":
      return {
        sample: rustDatetime("2024-01-01T00:00:00.000Z"),
        next: rustDatetime("2024-01-02T00:00:00.000Z"),
      };
    case "String":
      if (fieldType === "decimal") {
        return { sample: rustString("0"), next: rustString("1") };
      }
      if (fieldType === "uuid") {
        return {
          sample: rustString("00000000-0000-0000-0000-000000000000"),
          next: rustString("00000000-0000-0000-0000-000000000001"),
        };
      }
      if (fieldType === "datetime") {
        return {
          sample: rustString("2024-01-01T00:00:00.000Z"),
          next: rustString("2024-01-02T00:00:00.000Z"),
        };
      }
      if (fieldType === "date") {
        return {
          sample: rustString("2024-01-01"),
          next: rustString("2024-01-02"),
        };
      }
      if (fieldType === "email") {
        return {
          sample: rustString("sample@example.com"),
          next: rustString("next@example.com"),
        };
      }
      return { sample: rustString("sample"), next: rustString("sample-next") };
    default:
      throw new Error(`Unknown rust native type: ${native}`);
  }
};

export const wrapOption = (expr: string, nullable: boolean): string =>
  nullable ? `Some(${expr})` : expr;

export const sampleForField = (type: string, isNullable: boolean): string => {
  const { sample } = samplesForNative(convertSpecType(type), type);
  return wrapOption(sample, isNullable);
};
