import { toCase } from "@deterministic-code/generator-sdk/case";
import type { RawTypeDef } from "@deterministic-code/generator-sdk/deterministic-shapes";
import { systemColumnsInjectedFor } from "./system-columns.ts";
import { STANDARD_COLUMN_DEFS } from "./rust-standard-columns.ts";
import { datasourceValidatorGenerator } from "@deterministic-code/generator-sdk/codegen-context";
import type { CodegenNames } from "@deterministic-code/generator-sdk/codegen-naming";
import type { CodegenFieldNames } from "@deterministic-code/generator-sdk/field-names";
import type { CodegenLayout } from "@deterministic-code/generator-sdk/codegen-layout";
import { RustImports } from "./rust-imports.ts";
import { validatorOptionsFromSettings } from "@deterministic-code/generator-sdk/codegen/lib/generate-settings-options";
import {
  entryOf,
  isFiniteInt,
  isFiniteNumber,
} from "@deterministic-code/generator-sdk/generator-shared";
import type {
  DatasourceValidatorGenerateConfig,
  GeneratedFile,
  ValidatorFieldDef,
} from "@deterministic-code/generator-sdk/codegen/lib/datasource-validator-generate-types";

interface RustTableDef {
  fields: Array<Record<string, unknown>>;
}

interface RustGenerateOptions {
  idType: string;
  schemaVersion: string;
}

interface RustCtx {
  opts: RustGenerateOptions;
  names: CodegenNames;
  fields: CodegenFieldNames;
  layout: CodegenLayout;
  imports: { dsType(name: string): string };
  byFeature: boolean;
}

interface CheckArgs {
  fdef: ValidatorFieldDef;
  fieldName: string;
  propName: string;
  idType: string;
}

type RawCheckArgs = CheckArgs & { ref: string };

export const DEFAULT_GENERATE_OPTIONS = {
  schemaVersion: "1.0",
};

const INT_SUFFIX_BY_TYPE: Record<string, string> = {
  number: "i64",
  biginteger: "i64",
  reference: "i64",
  integer: "i32",
  smallinteger: "i16",
};

const NUMERIC_ID_SUFFIX: Record<string, string> = {
  i32: "i32",
  i64: "i64",
  integer: "i64",
  biginteger: "i64",
};

function floatLiteral(value: number): string {
  const s = String(value);
  return s.includes(".") ? `${s}f64` : `${s}.0f64`;
}

function stringChecks(
  fdef: ValidatorFieldDef,
  propName: string,
  ref: string,
): string[] {
  const out: string[] = [];
  if (isFiniteInt(fdef.min_size) && fdef.min_size! >= 0) {
    out.push(
      `if ${ref}.chars().count() < ${fdef.min_size} { errors.push("${propName}: must be at least ${fdef.min_size} chars".to_string()); }`,
    );
  }
  if (isFiniteInt(fdef.size) && fdef.size! >= 0) {
    out.push(
      `if ${ref}.chars().count() > ${fdef.size} { errors.push("${propName}: exceeds ${fdef.size} chars".to_string()); }`,
    );
  }
  return out;
}

function numericChecks({
  fdef,
  fieldName,
  propName,
  ref,
  idType,
}: RawCheckArgs): string[] {
  const suffix =
    fieldName === "id"
      ? NUMERIC_ID_SUFFIX[idType]
      : INT_SUFFIX_BY_TYPE[fdef.type];
  const isFk =
    typeof fdef.references === "string" && fdef.references.length > 0;
  const isIdLike = fieldName === "id" || fieldName.endsWith("_id");
  const out: string[] = [];
  if (isFk || isIdLike) {
    out.push(
      `if ${ref} < 0${suffix} { errors.push("${propName}: must be nonnegative".to_string()); }`,
    );
  } else if (isFiniteInt(fdef.min_size)) {
    out.push(
      `if ${ref} < ${fdef.min_size}${suffix} { errors.push("${propName}: must be at least ${fdef.min_size}".to_string()); }`,
    );
  }
  if (isFiniteInt(fdef.size)) {
    out.push(
      `if ${ref} > ${fdef.size}${suffix} { errors.push("${propName}: exceeds ${fdef.size}".to_string()); }`,
    );
  }
  return out;
}

function floatChecks(
  fdef: ValidatorFieldDef,
  propName: string,
  ref: string,
): string[] {
  const out: string[] = [];
  if (isFiniteNumber(fdef.min_size)) {
    out.push(
      `if ${ref} < ${floatLiteral(fdef.min_size!)} { errors.push("${propName}: must be at least ${fdef.min_size}".to_string()); }`,
    );
  }
  if (isFiniteNumber(fdef.size)) {
    out.push(
      `if ${ref} > ${floatLiteral(fdef.size!)} { errors.push("${propName}: exceeds ${fdef.size}".to_string()); }`,
    );
  }
  return out;
}

function uuidChecks(propName: string, ref: string): string[] {
  return [
    `let hex_dashes = ${ref}.len() == 36 && ${ref}.chars().enumerate().all(|(i, c)| if i == 8 || i == 13 || i == 18 || i == 23 { c == '-' } else { c.is_ascii_hexdigit() });`,
    `if !hex_dashes { errors.push("${propName}: must be a uuid".to_string()); }`,
  ];
}

function rawChecksForField(args: RawCheckArgs): string[] {
  const { fdef, propName, ref } = args;
  switch (fdef.type) {
    case "string":
    case "character":
      return stringChecks(fdef, propName, ref);
    case "uuid":
      return uuidChecks(propName, ref);
    case "number":
    case "integer":
    case "smallinteger":
    case "biginteger":
    case "reference":
      return numericChecks(args);
    case "float":
      return floatChecks(fdef, propName, ref);
    default:
      return [];
  }
}

function indent(line: string, depth: number): string {
  return `${"    ".repeat(depth)}${line}`;
}

function idColumnCheckLines(propName: string, idType: string): string[] {
  if (idType === "uuid") {
    return uuidChecks(propName, `obj.${propName}.to_string()`).map((l) =>
      indent(l, 1),
    );
  }
  if (idType === "string") return [];
  return [
    indent(
      `if obj.${propName} < 0${NUMERIC_ID_SUFFIX[idType]} { errors.push("${propName}: must be nonnegative".to_string()); }`,
      1,
    ),
  ];
}

function checksForField({
  fieldName,
  fdef,
  propName,
  idType,
}: CheckArgs): string[] {
  const isNullable = fdef.is_nullable === true;
  const isStringLike = fdef.type === "string" || fdef.type === "uuid";
  const valueRef = isNullable ? "v" : `obj.${propName}`;
  const numericRef = isNullable ? "*v" : `obj.${propName}`;
  const ref = isStringLike ? valueRef : numericRef;
  const inner = rawChecksForField({ fdef, fieldName, propName, ref, idType });
  if (inner.length === 0) return [];
  if (!isNullable) return inner.map((l) => indent(l, 1));
  return [
    indent(`if let Some(v) = &obj.${propName} {`, 1),
    ...inner.map((l) => indent(l, 2)),
    indent(`}`, 1),
  ];
}

function standardColumnLines(tableDef: unknown, ctx: RustCtx): string[] {
  const lines: string[] = [];
  const injected = systemColumnsInjectedFor(tableDef as RawTypeDef);
  const dropUuidColumn = ctx.opts.idType === "uuid";
  for (const colName of ["id", "uuid", "created", "updated"] as const) {
    if (!injected.has(colName)) continue;
    if (colName === "uuid" && dropUuidColumn) continue;
    const propName = ctx.fields.name(colName);
    if (colName === "id") {
      for (const l of idColumnCheckLines(propName, ctx.opts.idType))
        lines.push(l);
      continue;
    }
    for (const l of checksForField({
      fieldName: colName,
      fdef: STANDARD_COLUMN_DEFS[colName] as ValidatorFieldDef,
      propName,
      idType: ctx.opts.idType,
    })) {
      lines.push(l);
    }
  }
  return lines;
}

function renderTable(
  tableEntry: Record<string, unknown>,
  ctx: RustCtx,
): GeneratedFile {
  const { names, opts } = ctx;
  const [tableName, tableDef] = entryOf(tableEntry);

  const lines = standardColumnLines(tableDef, ctx);
  for (const f of (tableDef as RustTableDef).fields) {
    const [fname, fdef] = entryOf(f);
    const propName = ctx.fields.name(fname);
    for (const line of checksForField({
      fieldName: fname,
      fdef: fdef as ValidatorFieldDef,
      propName,
      idType: opts.idType,
    })) {
      lines.push(line);
    }
  }

  const cls = ctx.imports.dsType(tableName);
  const fn = `validate_datasource_${toCase(tableName, "Snake")}`; // lint-generator-casing-allow: toCase
  const hasChecks = lines.length > 0;
  const declared = hasChecks ? `let mut errors` : `let errors`;
  const paramName = hasChecks ? "obj" : "_obj";
  const body = [
    `pub fn ${fn}(${paramName}: &${cls}) -> Result<(), Vec<String>> {`,
    `    ${declared}: Vec<String> = Vec::new();`,
    ...lines,
    `    if errors.is_empty() { Ok(()) } else { Err(errors) }`,
    `}`,
  ].join("\n");

  const header = `// schema-version: ${opts.schemaVersion}\n`;

  const flatName = `datasource_${names.fileBase(tableName, "datasource-validator")}_validator${names.ext}`;
  const path = ctx.byFeature
    ? ctx.layout.filePath(tableName, "datasource-validator")
    : flatName;
  return {
    path,
    content: `${header}${body}\n`,
  };
}

const baseCreateGenerator = datasourceValidatorGenerator(renderTable);

export const createGenerator = () => {
  const base = baseCreateGenerator(RustImports);
  return {
    generate: (config: DatasourceValidatorGenerateConfig) =>
      base.generate({
        ...DEFAULT_GENERATE_OPTIONS,
        ...validatorOptionsFromSettings(config.settings),
        ...config,
      }),
  };
};
