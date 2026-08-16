import { toCase, } from "@deterministic-code/generator-sdk/case";
import { datasourceTestsModule } from "@deterministic-code/generator-sdk/codegen/lib/emit-settings-options";
import { rustLayout } from "./rust-crate-paths.js";
import { rustImportsForOptions } from "./rust-imports.js";
import { enumerateInvalidMutations } from "@deterministic-code/generator-sdk/codegen/lib/fixture-builder";
import { rustInjectedSystemFields, rustIdFieldValue, } from "./rust-standard-columns.js";
import { datasourceSettingsFor, } from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import { entryOf } from "@deterministic-code/generator-sdk/emitter-shared";
export const DEFAULT_EMIT_OPTIONS = {
    schemaVersion: "1.0",
};
const RUST_FIELD_VALUES = {
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
function typeName(name, opts) {
    return rustLayout(opts).names.className(name);
}
function validatorFn(name) {
    return `validate_datasource_${toCase(name, "Snake")}`; // lint-emitter-casing-allow: toCase
}
function fieldCase(name, opts) {
    return toCase(name, opts.fieldFormat); // lint-emitter-casing-allow: toCase
}
function fileBase(name, opts) {
    return toCase(`${name}_validator_tests`, opts.fileFormat); // lint-emitter-casing-allow: toCase
}
function rustValueFor(type) {
    const v = RUST_FIELD_VALUES[type];
    if (!v)
        throw new Error(`Unknown field type: ${type}`);
    return v.sample;
}
function rustNextValueFor(type) {
    const v = RUST_FIELD_VALUES[type];
    if (!v)
        throw new Error(`Unknown field type: ${type}`);
    return v.next;
}
function allFields(tableDef, opts) {
    const declared = (tableDef.fields ?? []).map((f) => {
        const [fname, fdef] = entryOf(f);
        const def = fdef;
        return {
            name: fname,
            type: def.type,
            isNullable: def.is_nullable === true,
        };
    });
    const ds = datasourceSettingsFor(opts);
    return [
        ...rustInjectedSystemFields(tableDef, ds),
        ...declared,
    ];
}
function fieldInit(field, opts, nullableVariant) {
    const name = fieldCase(field.name, opts);
    if (field.isNullable && nullableVariant)
        return `        ${name}: None,`;
    const base = field.name === "id"
        ? rustIdFieldValue(datasourceSettingsFor(opts))
        : rustValueFor(field.type);
    const value = field.isNullable ? `Some(${base})` : base;
    return `        ${name}: ${value},`;
}
function structLiteral(ctx, nullableVariant) {
    const { cls, tableDef, opts } = ctx;
    const fields = allFields(tableDef, opts).map((f) => fieldInit(f, opts, nullableVariant));
    if (fields.length === 0) {
        return `${cls} {}`;
    }
    return [`${cls} {`, ...fields, `    }`].join("\n");
}
function hasAnyNullable(tableDef, opts) {
    return allFields(tableDef, opts).some((f) => f.isNullable);
}
function renderStructAssertTest(fn, ctx, opts) {
    const assertion = opts.ok ? "is_ok" : "is_err";
    return [
        `    #[test]`,
        `    fn ${opts.testName}() {`,
        `        let value = ${structLiteral(ctx, opts.nullableVariant)};`,
        `        assert!(${fn}(&value).${assertion}());`,
        `    }`,
    ].join("\n");
}
function renderValidTest(fn, ctx) {
    return renderStructAssertTest(fn, ctx, {
        testName: "parses_a_valid_payload",
        nullableVariant: false,
        ok: true,
    });
}
function renderNullableTest(fn, ctx) {
    return renderStructAssertTest(fn, ctx, {
        testName: "accepts_null_for_nullable_fields",
        nullableVariant: true,
        ok: true,
    });
}
function renderGetsAndSetsTest(field, ctx) {
    const { opts } = ctx;
    const name = fieldCase(field.name, opts);
    const nextBase = field.name === "id"
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
function sanitizeMutationName(description) {
    return description
        .replace(/"/g, "")
        .split(/[^A-Za-z0-9]+/)
        .filter((t) => t.length > 0)
        .map((t) => t.toLowerCase())
        .join("_");
}
function classifyMutation(description) {
    if (/^missing required field/.test(description))
        return "missing";
    if (/^null for non-nullable field/.test(description))
        return "null_nonnullable";
    if (/^wrong type on field/.test(description))
        return "wrong_type";
    return "unknown";
}
function isMutationExpressibleInRust(description) {
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
function renderRejectsTest(fn, ctx, description) {
    return renderStructAssertTest(fn, ctx, {
        testName: `rejects_when_${sanitizeMutationName(description)}`,
        nullableVariant: false,
        ok: false,
    });
}
function renderAllowsNoneTest(field, ctx) {
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
function renderAccessorTests(ctx) {
    const fields = allFields(ctx.tableDef, ctx.opts);
    const tests = fields.map((field) => renderGetsAndSetsTest(field, ctx));
    for (const field of fields) {
        if (field.isNullable)
            tests.push(renderAllowsNoneTest(field, ctx));
    }
    return tests;
}
export function emitForTable(entry, datasource, options = DEFAULT_EMIT_OPTIONS) {
    const opts = { ...DEFAULT_EMIT_OPTIONS, ...options };
    const [tableName, tableDefRaw] = entryOf(entry);
    const tableDef = tableDefRaw;
    const cls = typeName(tableName, opts);
    const fn = validatorFn(tableName);
    const ctx = { cls, tableDef, opts };
    const tests = [renderValidTest(fn, ctx)];
    if (hasAnyNullable(tableDef, opts)) {
        tests.push(renderNullableTest(fn, ctx));
    }
    if (datasource) {
        const mutations = enumerateInvalidMutations({
            table: tableName,
            datasource,
        });
        for (const m of mutations) {
            if (!isMutationExpressibleInRust(m.description))
                continue;
            tests.push(renderRejectsTest(fn, ctx, m.description));
        }
    }
    tests.push(...renderAccessorTests(ctx));
    return assembleTestFile(tableName, tests, opts);
}
/** Wrap the rendered `#[cfg(test)] mod tests` bodies in the module header and resolve the (by-feature-aware) emitted path. */
function assembleTestFile(tableName, tests, opts) {
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
export const { emitFromSchema, createEmitter } = datasourceTestsModule({
    emitForTable,
    defaultEmitOptions: DEFAULT_EMIT_OPTIONS,
});
