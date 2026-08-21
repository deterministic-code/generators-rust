import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import type { PackCasing } from "./common/default-casing.ts";
import {
  DeterministicParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceField,
  type DatasourceType,
  type IDeterministic,
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
  field: DatasourceField,
  prop: string,
  ref: string,
): string[] => {
  const { type, name, minSize, size, references } = field;
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
          size !== undefined && size >= 0,
          `${ref}.chars().count() > ${size}`,
          `${prop}: exceeds ${size} chars`,
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
          size !== undefined,
          `${ref} > ${size}${suffix}`,
          `${prop}: exceeds ${size}`,
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
          size !== undefined,
          `${ref} > ${floatLit(size!)}`,
          `${prop}: exceeds ${size}`,
        ],
      ]);
    default:
      return [];
  }
};

const pad = (n: number, line: string): string => `${"    ".repeat(n)}${line}`;

const checksForField = (
  field: DatasourceField,
  casing: PackCasing,
  isStandardId = false,
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
    return deterministic.expandedDatasourceTypes.map((table) =>
      this.validator(table),
    );
  }

  private validator(table: DatasourceType): GenerateEntry {
    const lines = table.fields.flatMap((field) =>
      checksForField(field, this.casing, field.name === "id"),
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
  await ctx.reader.read(DATASOURCE_TYPES_YAML);
  return new Generator(ctx.settings).from(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  );
};
