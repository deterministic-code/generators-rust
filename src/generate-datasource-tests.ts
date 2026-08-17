import {
  toCase,
  type CaseFormat,
} from "@deterministic-code/generator-sdk/case";
import { datasourceTestsModule } from "@deterministic-code/generator-sdk/codegen/lib/generate-settings-options";
import { rustLayout } from "./rust-crate-paths.ts";
import { rustImportsForOptions } from "./rust-imports.ts";
import { enumerateInvalidMutations } from "@deterministic-code/generator-sdk/codegen/lib/fixture-builder";
import {
  rustInjectedSystemFields,
  rustIdFieldValue,
} from "./rust-standard-columns.ts";
import {
  datasourceSettingsFor,
  type DatasourceOptions,
} from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import { entryOf } from "./common/yaml-entry.ts";
import type { NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";

type Flatten<T> = { [K in keyof T]: T[K] };

type RustGenerateOptions = Flatten<
  NamesForOptions & DatasourceOptions & { schemaVersion: string }
>;

interface RustFieldDef {
  type: string;
  is_nullable?: boolean;
}

interface RustTableDef {
  fields?: Array<Record<string, RustFieldDef>>;
}

interface RustField {
  name: string;
  type: string;
  isNullable: boolean;
}

interface RustCtx {
  cls: string;
  tableDef: RustTableDef;
  opts: RustGenerateOptions;
}

interface Mutation {
  description: string;
}

export const DEFAULT_GENERATE_OPTIONS: RustGenerateOptions = {
  schemaVersion: "1.0",
};

const RUST_FIELD_VALUES: Record<string, { sample: string; next: string }> = {
  string: { sample: `String::from("sample")`, next: `String::from("next")` },
  character: { sample: `String::from("sample")`, next: `String::from("next")` },
  decimal: { sample: `String::from("0.00")`, next: `String::from("1.00")` },
  uuid: {
    sample: `uuid::Uuid::new_v4().to_string()`,
    next: `uuid::Uuid::new_v4().to_string()`,
  },
  number: { sample: `1i64`, next: `42i64` },
  biginteger: { sample: `1i64`, next: `42i64` },
  reference: { sample: `1i64`, next: `42i64` },
  integer: { sample: `1i32`, next: `42i32` },
  smallinteger: { sample: `1i16`, next: `42i16` },
  float: { sample: `1.0f64`, next: `42.0f64` },
  boolean: { sample: `true`, next: `false` },
  datetime: {
    sample: `chrono::Utc::now()`,
    next: `chrono::Utc::now() + chrono::Duration::days(1)`,
  },
  binary: { sample: `Vec::<u8>::new()`, next: `vec![1u8]` },
};

function typeName(name: string, opts: RustGenerateOptions): string {
  return rustLayout(opts).names.className(name);
}

function validatorFn(name: string): string {
  return `validate_datasource_${toCase(name, "Snake")}`; // lint-generator-casing-allow: toCase
}

function fieldCase(name: string, opts: RustGenerateOptions): string {
  return toCase(name, opts.fieldFormat as CaseFormat); // lint-generator-casing-allow: toCase
}

function fileBase(name: string, opts: RustGenerateOptions): string {
  return toCase(`${name}_validator_tests`, opts.fileFormat as CaseFormat); // lint-generator-casing-allow: toCase
}

function rustValueFor(type: string): string {
  const v = RUST_FIELD_VALUES[type];
  if (!v) throw new Error(`Unknown field type: ${type}`);
  return v.sample;
}

function rustNextValueFor(type: string): string {
  const v = RUST_FIELD_VALUES[type];
  if (!v) throw new Error(`Unknown field type: ${type}`);
  return v.next;
}

function allFields(tableDef: RustTableDef, opts: RustGenerateOptions): RustField[] {
  const declared: RustField[] = (tableDef.fields ?? []).map((f) => {
    const [fname, fdef] = entryOf(f);
    const def = fdef as RustFieldDef;
    return {
      name: fname,
      type: def.type,
      isNullable: def.is_nullable === true,
    };
  });
  const ds = datasourceSettingsFor(opts);
  return [
    ...(rustInjectedSystemFields(tableDef, ds) as RustField[]),
    ...declared,
  ];
}

function fieldInit(
  field: RustField,
  opts: RustGenerateOptions,
  nullableVariant: boolean,
): string {
  const name = fieldCase(field.name, opts);
  if (field.isNullable && nullableVariant) return `        ${name}: None,`;
  const base =
    field.name === "id"
      ? rustIdFieldValue(datasourceSettingsFor(opts))
      : rustValueFor(field.type);
  const value = field.isNullable ? `Some(${base})` : base;
  return `        ${name}: ${value},`;
}

function structLiteral(ctx: RustCtx, nullableVariant: boolean): string {
  const { cls, tableDef, opts } = ctx;
  const fields = allFields(tableDef, opts).map((f) =>
    fieldInit(f, opts, nullableVariant),
  );
  if (fields.length === 0) {
    return `${cls} {}`;
  }
  return [`${cls} {`, ...fields, `    }`].join("\n");
}

function hasAnyNullable(
  tableDef: RustTableDef,
  opts: RustGenerateOptions,
): boolean {
  return allFields(tableDef, opts).some((f) => f.isNullable);
}

function renderStructAssertTest(
  fn: string,
  ctx: RustCtx,
  opts: { testName: string; nullableVariant: boolean; ok: boolean },
): string {
  const assertion = opts.ok ? "is_ok" : "is_err";
  return [
    `    #[test]`,
    `    fn ${opts.testName}() {`,
    `        let value = ${structLiteral(ctx, opts.nullableVariant)};`,
    `        assert!(${fn}(&value).${assertion}());`,
    `    }`,
  ].join("\n");
}

function renderValidTest(fn: string, ctx: RustCtx): string {
  return renderStructAssertTest(fn, ctx, {
    testName: "parses_a_valid_payload",
    nullableVariant: false,
    ok: true,
  });
}

function renderNullableTest(fn: string, ctx: RustCtx): string {
  return renderStructAssertTest(fn, ctx, {
    testName: "accepts_null_for_nullable_fields",
    nullableVariant: true,
    ok: true,
  });
}

function renderGetsAndSetsTest(field: RustField, ctx: RustCtx): string {
  const { opts } = ctx;
  const name = fieldCase(field.name, opts);
  const nextBase =
    field.name === "id"
      ? rustIdFieldValue(datasourceSettingsFor(opts), "next")
      : rustNextValueFor(field.type);
  const next = field.isNullable ? `Some(${nextBase})` : nextBase;
  return [
    `    #[test]`,
    `    fn gets_and_sets_${name}() {`,
    `        let mut value = ${structLiteral(ctx, false)};`,
    `        let next = ${next};`,
    `        value.${name} = next.clone();`,
    `        assert_eq!(value.${name}, next);`,
    `    }`,
  ].join("\n");
}

function sanitizeMutationName(description: string): string {
  return description
    .replace(/"/g, "")
    .split(/[^A-Za-z0-9]+/)
    .filter((t) => t.length > 0)
    .map((t) => t.toLowerCase())
    .join("_");
}

function classifyMutation(description: string): string {
  if (/^missing required field/.test(description)) return "missing";
  if (/^null for non-nullable field/.test(description))
    return "null_nonnullable";
  if (/^wrong type on field/.test(description)) return "wrong_type";
  return "unknown";
}

function isMutationExpressibleInRust(description: string): boolean {
  switch (classifyMutation(description)) {
    case "missing":
    case "null_nonnullable":
    case "wrong_type":
    case "unknown":
      return false;
    default:
      return false;
  }
}

function renderRejectsTest(
  fn: string,
  ctx: RustCtx,
  description: string,
): string {
  return renderStructAssertTest(fn, ctx, {
    testName: `rejects_when_${sanitizeMutationName(description)}`,
    nullableVariant: false,
    ok: false,
  });
}

function renderAllowsNoneTest(field: RustField, ctx: RustCtx): string {
  const { opts } = ctx;
  const name = fieldCase(field.name, opts);
  return [
    `    #[test]`,
    `    fn allows_setting_${name}_to_none() {`,
    `        let mut value = ${structLiteral(ctx, false)};`,
    `        value.${name} = None;`,
    `        assert!(value.${name}.is_none());`,
    `    }`,
  ].join("\n");
}

/** The per-field accessor tests: a `gets_and_sets_<field>` for every field, plus an `allows_setting_<field>_to_none` for each nullable one. */
function renderAccessorTests(ctx: RustCtx): string[] {
  const fields = allFields(ctx.tableDef, ctx.opts);
  const tests = fields.map((field) => renderGetsAndSetsTest(field, ctx));
  for (const field of fields) {
    if (field.isNullable) tests.push(renderAllowsNoneTest(field, ctx));
  }
  return tests;
}

export function generateForTable(
  entry: Record<string, unknown>,
  datasource: unknown,
  options: Partial<RustGenerateOptions> = DEFAULT_GENERATE_OPTIONS,
): { path: string; content: string } {
  const opts: RustGenerateOptions = { ...DEFAULT_GENERATE_OPTIONS, ...options };
  const [tableName, tableDefRaw] = entryOf(entry);
  const tableDef = tableDefRaw as RustTableDef;
  const cls = typeName(tableName, opts);
  const fn = validatorFn(tableName);
  const ctx: RustCtx = { cls, tableDef, opts };
  const tests = [renderValidTest(fn, ctx)];

  if (hasAnyNullable(tableDef, opts)) {
    tests.push(renderNullableTest(fn, ctx));
  }

  if (datasource) {
    const mutations: Mutation[] = enumerateInvalidMutations({
      table: tableName,
      datasource,
    });
    for (const m of mutations) {
      if (!isMutationExpressibleInRust(m.description)) continue;
      tests.push(renderRejectsTest(fn, ctx, m.description));
    }
  }

  tests.push(...renderAccessorTests(ctx));

  return assembleTestFile(tableName, tests, opts);
}

/** Wrap the rendered `#[cfg(test)] mod tests` bodies in the module header and resolve the (by-feature-aware) generated path. */
function assembleTestFile(
  tableName: string,
  tests: string[],
  opts: RustGenerateOptions,
): { path: string; content: string } {
  const imp = rustImportsForOptions(opts);
  const header = [
    `// schema-version: ${opts.schemaVersion}`,
    `#[cfg(test)]`,
    `mod tests {`,
    `    use ${imp.dsTypeModule(tableName)}::*;`,
    `    use ${imp.dsValidatorModule(tableName)}::*;`,
    ``,
  ].join("\n");
  const body = `${tests.join("\n\n")}\n}\n`;
  const path = rustLayout(opts).testPath(tableName, "datasource-type", {
    fileName: `${fileBase(tableName, opts)}.rs`,
  });
  return { path, content: `${header}${body}` };
}

export const { generateFromSchema, createGenerator } = datasourceTestsModule({
  generateForTable,
  defaultGenerateOptions: DEFAULT_GENERATE_OPTIONS,
});
