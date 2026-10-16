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
  decimal: {
    Numeric: token(spec.decimal.Numeric!, rustString, /^String::from\("(.*)"\)$/),
    String: quoted,
  },
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
      () => "uuid::Uuid::new_v4().to_string()",
      /^uuid::Uuid::new_v4\(\)\.to_string\(\)$/,
    ),
    Empty: token(
      spec.uuid.Empty!,
      () => "uuid::Uuid::nil().to_string()",
      /^uuid::Uuid::nil\(\)\.to_string\(\)$/,
    ),
    Uuid: token(spec.uuid.Uuid!, rustString, /^String::from\("(.*)"\)$/),
  },
  reference: {},
});

export const toNative = (specType: string): string =>
  typeConverter.convert(specType);

export const convertSpecType = toNative;

const isRustZeroExpr = (expr: string): boolean =>
  expr === "false" ||
  expr === "0" ||
  expr === "0.0" ||
  expr === "String::new()" ||
  expr === 'String::from("")' ||
  expr === "vec![]" ||
  expr === "None";

/** Spec default expression, or `undefined` when absent or the language zero. */
export const rustSpecDefaultExpr = (
  specType: string,
  field: {
    hasDefault?: boolean;
    defaultValue?: string | number | boolean | null;
    isNullable: boolean;
  },
): string | undefined => {
  if (field.hasDefault !== true) return undefined;
  const expr = defaultValueConverter.convert(
    specType,
    String(field.defaultValue),
  );
  if (expr === null) return undefined;
  if (field.isNullable) return `Some(${expr})`;
  return isRustZeroExpr(expr) ? undefined : expr;
};

export const rustLanguageZero = (rustType: string): string => {
  if (rustType.startsWith("Option<")) return "None";
  if (rustType.startsWith("Vec<")) return "vec![]";
  if (rustType.startsWith("HashMap<")) return "HashMap::new()";
  if (rustType === "bool") return "false";
  if (rustType === "String") return "String::new()";
  if (rustType === "chrono::DateTime<chrono::Utc>") {
    return "chrono::DateTime::<chrono::Utc>::UNIX_EPOCH";
  }
  if (rustType === "f64") return "0.0";
  if (
    rustType === "i16" ||
    rustType === "i32" ||
    rustType === "i64" ||
    rustType === "u16" ||
    rustType === "u32" ||
    rustType === "u64"
  ) {
    return "0";
  }
  return `${rustType}::default()`;
};
