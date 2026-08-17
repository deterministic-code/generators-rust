import { systemColumnsInjectedFor } from "../../lib/system-columns.js";
import { STANDARD_COLUMN_DEFS, STANDARD_COLUMN_ORDER, dropSystemUuidField, } from "@deterministic-code/generator-sdk/codegen/lib/standard-columns";
export { STANDARD_COLUMN_DEFS, STANDARD_COLUMN_ORDER, INLINED_VIEW_AUDIT_FIELDS, inlinedViewAuditFieldsExcluding, } from "@deterministic-code/generator-sdk/codegen/lib/standard-columns";
export function rustInjectedSystemFields(def, ds) {
    const injected = systemColumnsInjectedFor(def);
    const declaredNames = rawDeclaredNameSet(def);
    const out = [];
    for (const name of STANDARD_COLUMN_ORDER) {
        if (!injected.has(name))
            continue;
        if (declaredNames.has(name))
            continue;
        out.push({ name, ...STANDARD_COLUMN_DEFS[name] });
    }
    return dropSystemUuidField(out, ds);
}
function rawDeclaredNameSet(def) {
    const out = new Set();
    const fields = Array.isArray(def?.fields) ? def.fields : [];
    for (const entry of fields) {
        if (!entry || typeof entry !== "object")
            continue;
        const key = Object.keys(entry)[0];
        if (key)
            out.add(key);
    }
    return out;
}
/** The Rust struct-literal value for the primary-key `id` field under this id_type, matching the struct the datasource-types generator renders via `rustIdType()`: a real `uuid::Uuid`, a `String`, or an `i64`. `variant: "next"` yields a distinct second value so `gets_and_sets` tests aren't tautologies (two `new_v4()`s differ). */
export function rustIdFieldValue(ds, variant = "sample") {
    switch (ds.rustIdType()) {
        case "uuid::Uuid":
            return "uuid::Uuid::new_v4()";
        case "String":
            return variant === "next"
                ? `String::from("next")`
                : `String::from("sample")`;
        default:
            return variant === "next" ? `42i64` : `1i64`;
    }
}
