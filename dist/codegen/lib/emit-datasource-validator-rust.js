import { toCase } from "@deterministic-code/generator-sdk/case";
import { systemColumnsInjectedFor } from "../../lib/system-columns.js";
import { STANDARD_COLUMN_DEFS } from "./rust-standard-columns.js";
import { datasourceValidatorEmitter } from "@deterministic-code/generator-sdk/codegen-context";
import { RustImports } from "./rust-imports.js";
import { validatorOptionsFromSettings } from "@deterministic-code/generator-sdk/codegen/lib/emit-settings-options";
import { entryOf, isFiniteInt, isFiniteNumber, } from "@deterministic-code/generator-sdk/emitter-shared";
export const DEFAULT_EMIT_OPTIONS = {
    schemaVersion: "1.0",
};
const INT_SUFFIX_BY_TYPE = {
    number: "i64",
    biginteger: "i64",
    reference: "i64",
    integer: "i32",
    smallinteger: "i16",
};
const NUMERIC_ID_SUFFIX = {
    i32: "i32",
    i64: "i64",
    integer: "i64",
    biginteger: "i64",
};
function floatLiteral(value) {
    const s = String(value);
    return s.includes(".") ? `${s}f64` : `${s}.0f64`;
}
function stringChecks(fdef, propName, ref) {
    const out = [];
    if (isFiniteInt(fdef.min_size) && fdef.min_size >= 0) {
        out.push(`if ${ref}.chars().count() < ${fdef.min_size} { errors.push("${propName}: must be at least ${fdef.min_size} chars".to_string()); }`);
    }
    if (isFiniteInt(fdef.size) && fdef.size >= 0) {
        out.push(`if ${ref}.chars().count() > ${fdef.size} { errors.push("${propName}: exceeds ${fdef.size} chars".to_string()); }`);
    }
    return out;
}
function numericChecks({ fdef, fieldName, propName, ref, idType, }) {
    const suffix = fieldName === "id"
        ? NUMERIC_ID_SUFFIX[idType]
        : INT_SUFFIX_BY_TYPE[fdef.type];
    const isFk = typeof fdef.references === "string" && fdef.references.length > 0;
    const isIdLike = fieldName === "id" || fieldName.endsWith("_id");
    const out = [];
    if (isFk || isIdLike) {
        out.push(`if ${ref} < 0${suffix} { errors.push("${propName}: must be nonnegative".to_string()); }`);
    }
    else if (isFiniteInt(fdef.min_size)) {
        out.push(`if ${ref} < ${fdef.min_size}${suffix} { errors.push("${propName}: must be at least ${fdef.min_size}".to_string()); }`);
    }
    if (isFiniteInt(fdef.size)) {
        out.push(`if ${ref} > ${fdef.size}${suffix} { errors.push("${propName}: exceeds ${fdef.size}".to_string()); }`);
    }
    return out;
}
function floatChecks(fdef, propName, ref) {
    const out = [];
    if (isFiniteNumber(fdef.min_size)) {
        out.push(`if ${ref} < ${floatLiteral(fdef.min_size)} { errors.push("${propName}: must be at least ${fdef.min_size}".to_string()); }`);
    }
    if (isFiniteNumber(fdef.size)) {
        out.push(`if ${ref} > ${floatLiteral(fdef.size)} { errors.push("${propName}: exceeds ${fdef.size}".to_string()); }`);
    }
    return out;
}
function uuidChecks(propName, ref) {
    return [
        `let hex_dashes = ${ref}.len() == 36 && ${ref}.chars().enumerate().all(|(i, c)| if i == 8 || i == 13 || i == 18 || i == 23 { c == '-' } else { c.is_ascii_hexdigit() });`,
        `if !hex_dashes { errors.push("${propName}: must be a uuid".to_string()); }`,
    ];
}
function rawChecksForField(args) {
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
function indent(line, depth) {
    return `${"    ".repeat(depth)}${line}`;
}
function idColumnCheckLines(propName, idType) {
    if (idType === "uuid") {
        return uuidChecks(propName, `obj.${propName}.to_string()`).map((l) => indent(l, 1));
    }
    if (idType === "string")
        return [];
    return [
        indent(`if obj.${propName} < 0${NUMERIC_ID_SUFFIX[idType]} { errors.push("${propName}: must be nonnegative".to_string()); }`, 1),
    ];
}
function checksForField({ fieldName, fdef, propName, idType, }) {
    const isNullable = fdef.is_nullable === true;
    const isStringLike = fdef.type === "string" || fdef.type === "uuid";
    const valueRef = isNullable ? "v" : `obj.${propName}`;
    const numericRef = isNullable ? "*v" : `obj.${propName}`;
    const ref = isStringLike ? valueRef : numericRef;
    const inner = rawChecksForField({ fdef, fieldName, propName, ref, idType });
    if (inner.length === 0)
        return [];
    if (!isNullable)
        return inner.map((l) => indent(l, 1));
    return [
        indent(`if let Some(v) = &obj.${propName} {`, 1),
        ...inner.map((l) => indent(l, 2)),
        indent(`}`, 1),
    ];
}
function standardColumnLines(tableDef, ctx) {
    const lines = [];
    const injected = systemColumnsInjectedFor(tableDef);
    const dropUuidColumn = ctx.opts.idType === "uuid";
    for (const colName of ["id", "uuid", "created", "updated"]) {
        if (!injected.has(colName))
            continue;
        if (colName === "uuid" && dropUuidColumn)
            continue;
        const propName = ctx.fields.name(colName);
        if (colName === "id") {
            for (const l of idColumnCheckLines(propName, ctx.opts.idType))
                lines.push(l);
            continue;
        }
        for (const l of checksForField({
            fieldName: colName,
            fdef: STANDARD_COLUMN_DEFS[colName],
            propName,
            idType: ctx.opts.idType,
        })) {
            lines.push(l);
        }
    }
    return lines;
}
function renderTable(tableEntry, ctx) {
    const { names, opts } = ctx;
    const [tableName, tableDef] = entryOf(tableEntry);
    const lines = standardColumnLines(tableDef, ctx);
    for (const f of tableDef.fields) {
        const [fname, fdef] = entryOf(f);
        const propName = ctx.fields.name(fname);
        for (const line of checksForField({
            fieldName: fname,
            fdef: fdef,
            propName,
            idType: opts.idType,
        })) {
            lines.push(line);
        }
    }
    const cls = ctx.imports.dsType(tableName);
    const fn = `validate_datasource_${toCase(tableName, "Snake")}`; // lint-emitter-casing-allow: toCase
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
const baseCreateEmitter = datasourceValidatorEmitter(renderTable);
export const createEmitter = () => {
    const base = baseCreateEmitter(RustImports);
    return {
        emit: (config) => base.emit({
            ...DEFAULT_EMIT_OPTIONS,
            ...validatorOptionsFromSettings(config.settings),
            ...config,
        }),
    };
};
