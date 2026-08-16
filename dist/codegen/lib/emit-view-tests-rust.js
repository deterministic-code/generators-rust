import { toCase, pascal, snake } from "@deterministic-code/generator-sdk/case";
import { testCasingOptionsFromSettings } from "@deterministic-code/generator-sdk/codegen/lib/emit-settings-options";
import { normalizeAll } from "@deterministic-code/generator-sdk/view-expand";
import { viewEmitter } from "@deterministic-code/generator-sdk/codegen-context";
import { RustImports, rustImportsForOptions } from "./rust-imports.js";
import { rustInjectedSystemFields, inlinedViewAuditFieldsExcluding, rustIdFieldValue, } from "./rust-standard-columns.js";
import { datasourceSettingsFor, } from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import { classifyViewShape, declaredFieldsOf, } from "@deterministic-code/generator-sdk/codegen/lib/emit-view-shared";
export const DEFAULT_EMIT_OPTIONS = {
    schemaVersion: "1.0",
};
function fileBase(name, opts) {
    return toCase(`${name}_validator_tests`, opts.fileFormat); // lint-emitter-casing-allow: toCase
}
function indexDatasource(datasource, ds) {
    const index = new Map();
    for (const entry of datasource.types ?? []) {
        const [name, def] = Object.entries(entry)[0];
        const declared = declaredFieldsOf(def);
        const declaredNames = new Set(declared.map((f) => f.name));
        index.set(name, {
            name,
            declared,
            datasourceFields: [
                ...rustInjectedSystemFields(def, ds),
                ...declared,
            ],
            inlinedFields: [
                ...inlinedViewAuditFieldsExcluding(declaredNames, ds),
                ...declared,
            ],
        });
    }
    return index;
}
function primitiveExpr(base, useNext) {
    switch (base) {
        case "string":
            return useNext ? `String::from("next")` : `String::from("sample")`;
        case "uuid":
            return `uuid::Uuid::new_v4().to_string()`;
        case "number":
        case "reference":
            return useNext ? `42i64` : `1i64`;
        case "boolean":
            return useNext ? `false` : `true`;
        case "datetime":
            return useNext
                ? `chrono::Utc::now() + chrono::Duration::days(1)`
                : `chrono::Utc::now()`;
        case "binary":
            return useNext ? `vec![1u8]` : `Vec::<u8>::new()`;
        default:
            return `Default::default()`;
    }
}
function renderDatasourceField(field, ctx, useNext) {
    if (field.isNullable && ctx.nullableVariant)
        return "None";
    const base = field.name === "id"
        ? rustIdFieldValue(ctx.ds, useNext ? "next" : "sample")
        : primitiveExpr(field.type, useNext);
    return field.isNullable ? `Some(${base})` : base;
}
function renderDatasourceStruct(name, ctx) {
    const def = ctx.dsIndex.get(name);
    if (!def)
        throw new Error(`unknown datasource type: ${name}`);
    const cls = ctx.imports.dsType(name);
    const lines = def.datasourceFields.map((f) => {
        const val = renderDatasourceField(f, ctx, false);
        return `        ${snake(f.name)}: ${val},`; // lint-emitter-casing-allow: snake
    });
    if (lines.length === 0)
        return `${cls} {}`;
    return [`${cls} {`, ...lines, `    }`].join("\n");
}
function renderViewFieldValue(field, ctx, useNext) {
    const { parsed } = field;
    const makeElement = () => {
        if (parsed.kind === "primitive") {
            return primitiveExpr(parsed.base, useNext);
        }
        if (parsed.kind === "datasource") {
            return renderDatasourceStruct(parsed.base, ctx);
        }
        return renderViewStruct(parsed.base, ctx);
    };
    if (parsed.isArray) {
        if (field.isNullable && ctx.nullableVariant)
            return "None";
        const elem = makeElement();
        const arr = `vec![${elem}]`;
        return field.isNullable ? `Some(${arr})` : arr;
    }
    if (field.isNullable && ctx.nullableVariant)
        return "None";
    const value = makeElement();
    return field.isNullable ? `Some(${value})` : value;
}
function pushFieldLine(args) {
    const { lines, emitted, viewName, fieldName, line } = args;
    if (emitted.has(fieldName)) {
        throw new Error(`emit-view-tests-rust: duplicate field "${fieldName}" in struct literal for view "${viewName}". ` +
            `This indicates a field-name collision the validator should have caught (likely a view declares a ` +
            `field with the same name as one of its inherited datasource's fields, or an auto-injected ` +
            `column was unintentionally re-emitted by codegen). Fix the validator gate or the view's YAML.`);
    }
    emitted.add(fieldName);
    lines.push(line);
}
/** Push the inherited datasource's inlined fields (minus `omit`) as struct-literal lines. */
function pushInlinedParentFields(args) {
    const { lines, emitted, name, ctx, inherits, omit } = args;
    const def = ctx.dsIndex.get(inherits);
    if (!def)
        return;
    for (const f of def.inlinedFields) {
        if (omit.has(f.name))
            continue;
        const val = renderDatasourceField(f, ctx, false);
        pushFieldLine({
            lines,
            emitted,
            viewName: name,
            fieldName: f.name,
            line: `        ${snake(f.name)}: ${val},`, // lint-emitter-casing-allow: snake
        });
    }
}
/** The struct-literal field lines for a shaped view: inherited (inlined/omit/base) columns then declared fields. */
function viewStructFieldLines(args) {
    const { view, name, ctx, shape } = args;
    const { enrichments, inlineParent, inlineForOmit, omitList } = shape;
    const lines = [];
    const emitted = new Set();
    const inlined = (omit) => pushInlinedParentFields({
        lines,
        emitted,
        name,
        ctx,
        inherits: view.inherits,
        omit,
    });
    if (view.inherits && inlineParent) {
        inlined(new Set(enrichments.map((e) => e.fkColumn)));
    }
    else if (view.inherits && inlineForOmit) {
        inlined(new Set(omitList));
    }
    else if (view.inherits) {
        lines.push(`        base: ${renderDatasourceStruct(view.inherits, ctx)},`);
        emitted.add("base");
    }
    for (const f of view.fields) {
        const val = renderViewFieldValue(f, ctx, false);
        pushFieldLine({
            lines,
            emitted,
            viewName: name,
            fieldName: f.name,
            line: `        ${snake(f.name)}: ${val},`, // lint-emitter-casing-allow: snake
        });
    }
    return lines;
}
function renderViewStruct(name, ctx) {
    if (ctx.visited.has(`view:${name}`)) {
        throw new Error(`cyclic view reference: ${name}`);
    }
    const view = ctx.viewIndex.get(name);
    if (!view)
        throw new Error(`unknown view: ${name}`);
    ctx.visited.add(`view:${name}`);
    try {
        if (view.kind === "union") {
            const memberExpr = renderViewStruct(view.members[0], ctx);
            return `${ctx.imports.viewType(name)}::${pascal(view.members[0])}(${memberExpr})`; // lint-emitter-casing-allow: pascal
        }
        const shape = classifyViewShape(view);
        if (shape.aliasParent)
            return renderDatasourceStruct(view.inherits, ctx);
        const cls = ctx.imports.viewType(name);
        const lines = viewStructFieldLines({ view, name, ctx, shape });
        if (lines.length === 0)
            return `${cls} {}`;
        return [`${cls} {`, ...lines, `    }`].join("\n");
    }
    finally {
        ctx.visited.delete(`view:${name}`);
    }
}
function hasAnyNullable(view) {
    return view.fields.some((f) => f.isNullable);
}
function renderShapedTests(view, ctx) {
    const validExpr = renderViewStruct(view.name, {
        ...ctx,
        nullableVariant: false,
    });
    const tests = [
        [
            `    #[test]`,
            `    fn parses_a_valid_payload() {`,
            `        let value = ${validExpr};`,
            `        assert!(${ctx.imports.viewValidator(view.name)}(&value).is_ok());`,
            `    }`,
        ].join("\n"),
    ];
    if (hasAnyNullable(view)) {
        const nullableExpr = renderViewStruct(view.name, {
            ...ctx,
            nullableVariant: true,
        });
        tests.push([
            `    #[test]`,
            `    fn accepts_null_for_nullable_fields() {`,
            `        let value = ${nullableExpr};`,
            `        assert!(${ctx.imports.viewValidator(view.name)}(&value).is_ok());`,
            `    }`,
        ].join("\n"));
    }
    tests.push(...fieldMutationTests({ view, validExpr, ctx }));
    return tests;
}
/** Per-field `gets_and_sets_*` mutation tests, plus `allows_setting_*_to_none` for each nullable field. */
function fieldMutationTests(args) {
    const { view, validExpr, ctx } = args;
    const tests = [];
    for (const field of view.fields) {
        const name = snake(field.name); // lint-emitter-casing-allow: snake
        const nextExpr = renderViewFieldValue(field, { ...ctx, nullableVariant: false }, true);
        tests.push([
            `    #[test]`,
            `    fn gets_and_sets_${name}() {`,
            `        let mut value = ${validExpr};`,
            `        let next = ${nextExpr};`,
            `        value.${name} = next.clone();`,
            `        assert_eq!(value.${name}, next);`,
            `    }`,
        ].join("\n"));
    }
    for (const field of view.fields) {
        if (!field.isNullable)
            continue;
        const name = snake(field.name); // lint-emitter-casing-allow: snake
        tests.push([
            `    #[test]`,
            `    fn allows_setting_${name}_to_none() {`,
            `        let mut value = ${validExpr};`,
            `        value.${name} = None;`,
            `        assert!(value.${name}.is_none());`,
            `    }`,
        ].join("\n"));
    }
    return tests;
}
function renderUnionTests(view, ctx) {
    const tests = [];
    for (const member of view.members) {
        const memberFn = `accepts_${snake(member)}_member`; // lint-emitter-casing-allow: snake
        const memberExpr = `${ctx.imports.viewType(view.name)}::${pascal(member)}(${renderViewStruct(member, ctx)})`; // lint-emitter-casing-allow: pascal
        tests.push([
            `    #[test]`,
            `    fn ${memberFn}() {`,
            `        let value = ${memberExpr};`,
            `        assert!(${ctx.imports.viewValidator(view.name)}(&value).is_ok());`,
            `    }`,
        ].join("\n"));
    }
    return tests;
}
export function emitForView(view, { datasource, viewIndex, }, options = {}) {
    const opts = { ...DEFAULT_EMIT_OPTIONS, ...options };
    const ds = datasourceSettingsFor(opts);
    const ctx = {
        dsIndex: indexDatasource(datasource, ds),
        viewIndex,
        visited: new Set(),
        nullableVariant: false,
        imports: rustImportsForOptions(opts),
        ds,
    };
    const tests = view.kind === "union"
        ? renderUnionTests(view, ctx)
        : renderShapedTests(view, ctx);
    const header = [
        `// schema-version: ${opts.schemaVersion}`,
        `#[cfg(test)]`,
        `mod tests {`,
        ``,
    ].join("\n");
    const body = `${tests.join("\n\n")}\n}\n`;
    return {
        path: `${fileBase(view.name, opts)}.rs`,
        content: `${header}${body}`,
    };
}
export function emitFromSchema({ viewTypes, datasource }, options = DEFAULT_EMIT_OPTIONS) {
    const opts = { ...DEFAULT_EMIT_OPTIONS, ...options };
    const normalized = normalizeAll(viewTypes);
    const viewIndex = new Map(normalized.map((v) => [v.name, v]));
    return normalized.map((v) => emitForView(v, { datasource, viewIndex }, opts));
}
const baseCreateEmitter = viewEmitter((view, ctx) => {
    const viewIndex = (ctx.viewTestIndex ??= new Map(normalizeAll(ctx.opts.viewTypes).map((v) => [v.name, v])));
    const file = emitForView(view, { datasource: ctx.opts.datasourceTypes, viewIndex }, ctx.opts);
    if (!ctx.byFeature)
        return file;
    const fileName = `${ctx.names.fileBase(view.name, "view-validator")}_tests${ctx.names.ext}`;
    return {
        ...file,
        path: ctx.layout.testPath(view.name, "view-validator", { fileName }),
    };
});
export const createEmitter = () => {
    const base = baseCreateEmitter(RustImports);
    return {
        emit: (config) => base.emit({
            ...DEFAULT_EMIT_OPTIONS,
            ...testCasingOptionsFromSettings(config),
            ...config,
        }),
    };
};
