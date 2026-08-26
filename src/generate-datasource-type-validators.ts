import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  columnFields,
  datasourceTypesOf,
  isPkField,
} from "@deterministic-code/generators-common/spec-types";
import type { PackCasing } from "./common/default-casing.ts";
import {
  DeterministicParser,
  TYPES_YAML,
  type IDeterministic,
  type Type,
  type TypeField,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTmpl } from "./resources/datasource-type-validators.ts";
import { Emit } from "./emit.ts";

const errIf = (cond: string, msg: string): string =>
  `if ${cond} { errors.push("${msg}".to_string()); }`;

const guard = (rows: Array<[boolean, string, string]>): string[] =>
  rows.flatMap(([ok, cond, msg]) => (ok ? [errIf(cond, msg)] : []));

const floatLit = (n: number): string => {
  const s = String(n);
  return s.includes(".") ? `${s}f64` : `${s}.0f64`;
};

const uuidChecks = (prop: string, ref: string): string[] => [
  `let hex_dashes = ${ref}.len() == 36 && ${ref}.chars().enumerate().all(|(i, c)| if i == 8 || i == 13 || i == 18 || i == 23 { c == '-' } else { c.is_ascii_hexdigit() });`,
  `if !hex_dashes { errors.push("${prop}: must be a uuid".to_string()); }`,
];

const rawChecks = (
  field: TypeField,
  prop: string,
  ref: string,
): string[] => {
  const { type, name, minSize, size, references } = field;
  const max = typeof size === "number" ? size : undefined;
  switch (type) {
    case "string":
    case "character":
      return guard([
        [
          minSize !== undefined && minSize >= 0,
          `${ref}.chars().count() < ${minSize}`,
          `${prop}: must be at least ${minSize} chars`,
        ],
        [
          max !== undefined && max >= 0,
          `${ref}.chars().count() > ${max}`,
          `${prop}: exceeds ${max} chars`,
        ],
      ]);
    case "uuid":
      return uuidChecks(prop, ref);
    case "number":
    case "integer":
    case "smallinteger":
    case "biginteger":
    case "reference": {
      const suffix = convertSpecType(type);
      const idLike =
        name === "id" ||
        name.endsWith("_id") ||
        (typeof references === "string" && references.length > 0);
      return guard([
        [idLike, `${ref} < 0${suffix}`, `${prop}: must be nonnegative`],
        [
          !idLike && minSize !== undefined,
          `${ref} < ${minSize}${suffix}`,
          `${prop}: must be at least ${minSize}`,
        ],
        [
          max !== undefined,
          `${ref} > ${max}${suffix}`,
          `${prop}: exceeds ${max}`,
        ],
      ]);
    }
    case "float":
      return guard([
        [
          minSize !== undefined,
          `${ref} < ${floatLit(minSize!)}`,
          `${prop}: must be at least ${minSize}`,
        ],
        [
          max !== undefined,
          `${ref} > ${floatLit(max ?? 0)}`,
          `${prop}: exceeds ${max}`,
        ],
      ]);
    default:
      return [];
  }
};

const pad = (n: number, line: string): string => `${"    ".repeat(n)}${line}`;

const checksForField = (
  field: TypeField,
  casing: PackCasing,
  isStandardId: boolean,
): string[] => {
  const prop = casing.convertFields(field.name);
  if (isStandardId) {
    if (field.type === "string") return [];
    if (field.type === "uuid") {
      return uuidChecks(prop, `obj.${prop}.to_string()`).map((l) => pad(1, l));
    }
    return [
      pad(
        1,
        errIf(
          `obj.${prop} < 0${convertSpecType(field.type)}`,
          `${prop}: must be nonnegative`,
        ),
      ),
    ];
  }
  const stringLike = field.type === "string" || field.type === "uuid";
  const ref = field.isNullable ? (stringLike ? "v" : "*v") : `obj.${prop}`;
  const inner = rawChecks(field, prop, ref);
  if (inner.length === 0) return [];
  if (!field.isNullable) return inner.map((l) => pad(1, l));
  return [
    pad(1, `if let Some(v) = &obj.${prop} {`),
    ...inner.map((l) => pad(2, l)),
    pad(1, `}`),
  ];
};

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    return datasourceTypesOf(deterministic).map((table) =>
      this.validator(table),
    );
  }

  private validator(table: Type): GenerateEntry {
    const lines = columnFields(table.fields).flatMap((field) => {
      checksForField(field, this.casing, isPkField(field, table)),
    );
    const has = lines.length > 0;
    return content(
      this.imports.datasourceValidator(table.name),
      fill(typeTmpl, {
        schemaVersion: this.settings.schemaVersion,
        fnName: this.casing.convertFields(`validate_datasource_${table.name}`),
        paramName: has ? "obj" : "_obj",
        typePath: this.imports.datasourceQual(table.name),
        declared: has ? "let mut errors" : "let errors",
        checks: lines.map((line) => ({ line })),
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(TYPES_YAML);
  return new Generator(ctx.settings).from(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  );
};
