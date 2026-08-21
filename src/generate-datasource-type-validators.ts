import { snakeCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  createImportGenerator,
  type RustImportGenerator,
} from "./import-generator.ts";
import {
  DeterministicParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceField,
  type ExpandedDatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { isFiniteInt, isFiniteNumber } from "@deterministic-code/generators-common/yaml-entry";
import { typeTmpl } from "./resources/datasource-type-validators.ts";

type EmitOptions = {
  imports: RustImportGenerator;
  schemaVersion: string;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  imports: createImportGenerator(".", settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
});

const fieldName = (field: string): string => field;

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
          isFiniteInt(minSize) && minSize! >= 0,
          `${ref}.chars().count() < ${minSize}`,
          `${prop}: must be at least ${minSize} chars`,
        ],
        [
          isFiniteInt(size) && size! >= 0,
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
          !idLike && isFiniteInt(minSize),
          `${ref} < ${minSize}${suffix}`,
          `${prop}: must be at least ${minSize}`,
        ],
        [
          isFiniteInt(size),
          `${ref} > ${size}${suffix}`,
          `${prop}: exceeds ${size}`,
        ],
      ]);
    }
    case "float":
      return guard([
        [
          isFiniteNumber(minSize),
          `${ref} < ${floatLit(minSize!)}`,
          `${prop}: must be at least ${minSize}`,
        ],
        [
          isFiniteNumber(size),
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
  opts: EmitOptions,
  isStandardId = false,
): string[] => {
  const prop = fieldName(field.name);
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

const validatorPath = (entity: string, imports: RustImportGenerator): string =>
  imports.datasourceValidator(entity);

const typePath = (entity: string, imports: RustImportGenerator): string =>
  imports.datasourceQual(entity);

const renderValidator = (
  table: ExpandedDatasourceType,
  opts: EmitOptions,
): GenerateEntry => {
  const lines = table.fields.flatMap((field) =>
    checksForField(field, opts, field.name === "id"),
  );
  const has = lines.length > 0;
  return content(
    validatorPath(table.name, opts.imports),
    fill(typeTmpl, {
      schemaVersion: opts.schemaVersion,
      fnName: `validate_datasource_${snakeCase(table.name)}`,
      paramName: has ? "obj" : "_obj",
      typePath: typePath(table.name, opts.imports),
      declared: has ? "let mut errors" : "let errors",
      checks: lines.map((line) => ({ line })),
    }),
  );
};

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const opts = emitOptions(settings);
  return deterministic.expandedDatasourceTypes.map((table) =>
    renderValidator(table, opts),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(DATASOURCE_TYPES_YAML);
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
    ctx.settings,
  );
};
