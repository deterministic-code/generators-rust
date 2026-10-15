import {
  DeterministicDefaultValueConverter,
  type DefaultEntry,
  type SpecToken,
} from "@deterministic-code/generators-common/deterministic-default-value-converter";
import { DeterministicTypeConverter } from "@deterministic-code/generators-common/deterministic-type-converter";
import { hexToBytes } from "@deterministic-code/generators-common/default-token";

const spec = DeterministicDefaultValueConverter.spec;

const rustString = (value: string): string => {
  const escaped = value.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  return `String::from("${escaped}")`;
};

const token = (
  specToken: SpecToken,
  native: (arg: string) => string,
  reverse: RegExp,
): DefaultEntry => ({ ...specToken, native, reverse });

const quoted = token(spec.string.String!, rustString, /^String::from\("(.*)"\)$/);

const numeric = (type: string): Record<string, DefaultEntry> => {
  const numericSpec = spec[type]!.Numeric!;
  return {
    Numeric: token(numericSpec, (arg) => arg, numericSpec.spec),
    String: token(numericSpec, (arg) => arg, numericSpec.spec),
  };
};

export const typeConverter = new DeterministicTypeConverter({
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
});

export const defaultValueConverter = new DeterministicDefaultValueConverter({
  string: { String: quoted, Numeric: quoted },
  character: { String: quoted, Numeric: quoted },
  number: numeric("number"),
  integer: numeric("integer"),
  unsignedinteger: numeric("unsignedinteger"),
  smallinteger: numeric("smallinteger"),
  unsignedsmallinteger: numeric("unsignedsmallinteger"),
  biginteger: numeric("biginteger"),
  unsignedbiginteger: numeric("unsignedbiginteger"),
  float: numeric("float"),
  decimal: numeric("decimal"),
  boolean: {
    Boolean: token(
      spec.boolean.Boolean!,
      (arg) => (arg === "true" ? "true" : "false"),
      /^(true|false)$/,
    ),
  },
  datetime: {
    Now: token(
      spec.datetime.Now!,
      () => "chrono::Utc::now()",
      /^chrono::Utc::now\(\)$/,
    ),
    UtcNow: token(
      spec.datetime.UtcNow!,
      () => "chrono::Utc::now()",
      /^chrono::Utc::now\(\)$/,
    ),
    DateTime: token(
      spec.datetime.DateTime!,
      (arg) =>
        `"${arg}".parse::<chrono::DateTime<chrono::Utc>>().unwrap()`,
      /^"(.*)"\.parse::<chrono::DateTime<chrono::Utc>>\(\)\.unwrap\(\)$/,
    ),
  },
  binary: {
    Hex: token(
      spec.binary.Hex!,
      (arg) => `vec![${hexToBytes(arg).join(", ")}]`,
      /^vec!\[.*\]$/,
    ),
  },
  uuid: {
    NewId: token(
      spec.uuid.NewId!,
      () => "uuid::Uuid::new_v4()",
      /^uuid::Uuid::new_v4\(\)$/,
    ),
    Empty: token(
      spec.uuid.Empty!,
      () => "uuid::Uuid::nil()",
      /^uuid::Uuid::nil\(\)$/,
    ),
    Uuid: token(spec.uuid.Uuid!, rustString, /^String::from\("(.*)"\)$/),
  },
  reference: {},
});

export const toNative = (specType: string): string =>
  typeConverter.convert(specType);

export const convertSpecType = toNative;
