import { snakeCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { datasourcePaths, type ArtifactPaths } from "./common/paths.ts";
import {
  inheritedIdType,
  SpecificationParser,
  type DatasourceField,
  type DatasourceType,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { isFiniteInt, isFiniteNumber } from "@deterministic-code/generators-common/yaml-entry";
import { typeTmpl } from "./resources/datasource-type-validators.ts";
import { systemColumnsInjectedFor } from "./system-columns.ts";

type Datasource = {
  idType: string;
  withUuidColumn: boolean;
  useOptimisticConcurrency: boolean;
};

const datasource = (settings: Record<string, string>): Datasource => {
  const idType = settings["datasource.id_type"] ?? "integer";
  return {
    idType,
    withUuidColumn: idType !== "uuid",
    useOptimisticConcurrency:
      settings["datasource.use_optimistic_concurrency"] === "true",
  };
};

type EmitOptions = {
  ds: Datasource;
  naming: ArtifactPaths;
  schemaVersion: string;
};

const SYSTEM: Record<string, DatasourceField> = {
  id: { name: "id", type: "number", isNullable: false },
  uuid: { name: "uuid", type: "uuid", isNullable: false },
  created: { name: "created", type: "datetime", isNullable: false },
  updated: { name: "updated", type: "datetime", isNullable: false },
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  ds: datasource(settings),
  naming: datasourcePaths(settings),
  schemaVersion: settings["codegen.schema_version"] ?? "1.0",
});

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
  const prop = opts.naming.fieldName(field.name);
  const { idType } = opts.ds;
  if (isStandardId) {
    if (idType === "string") return [];
    if (idType === "uuid") {
      return uuidChecks(prop, `obj.${prop}.to_string()`).map((l) => pad(1, l));
    }
    return [
      pad(
        1,
        errIf(
          `obj.${prop} < 0${convertSpecType(inheritedIdType(idType))}`,
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

const standardLines = (table: DatasourceType, opts: EmitOptions): string[] => {
  const injected = systemColumnsInjectedFor({
    datasource_type: table.datasourceType,
    fields: table.fields.map((f) => ({
      [f.name]: f.isPrimaryKey ? { primary_key: true } : {},
    })),
  });
  return (["id", "uuid", "created", "updated"] as const)
    .filter((n) => injected.has(n) && (n !== "uuid" || opts.ds.withUuidColumn))
    .flatMap((n) => checksForField(SYSTEM[n], opts, n === "id"));
};

const validatorPath = (entity: string, naming: ArtifactPaths): string =>
  naming.byFeature
    ? naming.filePath(entity).replace(/\.rs$/, "_validator.rs")
    : `datasource_${naming.fileBase(entity)}_validator.rs`;

const typePath = (entity: string, naming: ArtifactPaths): string => {
  const cls = naming.className(entity);
  if (!naming.byFeature) return `crate::types::generated::datasource::${cls}`;
  const module = naming
    .filePath(entity)
    .replace(/\.rs$/, "")
    .replace(/^features\//, "")
    .replaceAll("/", "::");
  return `crate::features::${module}::${cls}`;
};

const renderValidator = (
  table: DatasourceType,
  opts: EmitOptions,
): GenerateEntry => {
  const lines = [
    ...standardLines(table, opts),
    ...table.fields.flatMap((field) => checksForField(field, opts)),
  ];
  const has = lines.length > 0;
  return content(
    validatorPath(table.name, opts.naming),
    fill(typeTmpl, {
      schemaVersion: opts.schemaVersion,
      fnName: `validate_datasource_${snakeCase(table.name)}`,
      paramName: has ? "obj" : "_obj",
      typePath: typePath(table.name, opts.naming),
      declared: has ? "let mut errors" : "let errors",
      checks: lines.map((line) => ({ line })),
    }),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const opts = emitOptions(ctx.settings);
  const types = await new SpecificationParser(ctx.reader).loadDatasourceTypes(opts.ds.idType);
  return types.map((table) => renderValidator(table, opts));
};
