import type { NativeInfo } from "@deterministic-code/generators-common/base-type-converter";
import { hexToBytes } from "@deterministic-code/generators-common/default-token";

const rustString = (value: string): string => {
  const escaped = value.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  return `String::from("${escaped}")`;
};

const numeric: NativeInfo["defaults"] = {
  Numeric: (arg: string) => arg,
  String: (arg: string) => arg,
};

const stringy: NativeInfo["defaults"] = {
  String: rustString,
  Numeric: rustString,
};

export const conversions: Record<string, NativeInfo> = {
  string: { to: "String", defaults: stringy },
  character: { to: "String", defaults: stringy },
  number: { to: "i64", defaults: numeric },
  integer: { to: "i32", defaults: numeric },
  unsignedinteger: { to: "u32", defaults: numeric },
  smallinteger: { to: "i16", defaults: numeric },
  unsignedsmallinteger: { to: "u16", defaults: numeric },
  biginteger: { to: "i64", defaults: numeric },
  unsignedbiginteger: { to: "u64", defaults: numeric },
  float: { to: "f64", defaults: numeric },
  decimal: { to: "String", defaults: numeric },
  boolean: {
    to: "bool",
    defaults: {
      Boolean: (arg: string) => (arg === "true" ? "true" : "false"),
    },
  },
  datetime: {
    to: "chrono::DateTime<chrono::Utc>",
    defaults: {
      Now: () => "chrono::Utc::now()",
      UtcNow: () => "chrono::Utc::now()",
      DateTime: (arg: string) =>
        `"${arg}".parse::<chrono::DateTime<chrono::Utc>>().unwrap()`,
    },
  },
  binary: {
    to: "Vec<u8>",
    defaults: {
      Hex: (arg: string) => `vec![${hexToBytes(arg).join(", ")}]`,
    },
  },
  uuid: {
    to: "String",
    defaults: {
      NewId: () => "uuid::Uuid::new_v4()",
      Empty: () => "uuid::Uuid::nil()",
      Uuid: rustString,
    },
  },
  reference: { to: "i64", defaults: {} },
};

export const toNative = (specType: string): string => {
  const info = conversions[specType];
  if (info === undefined) {
    throw new Error(`Unknown spec field type: ${specType}`);
  }
  return info.to;
};

export const convertSpecType = toNative;
