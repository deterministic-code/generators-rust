import { emitRoutesFiles, dispatchRoutesStep, routesStepEmit, } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit";
import { rustLayout, serviceUseLine, routeModulePath, appWiringFilePath, } from "./rust-crate-paths.js";
import { serviceFieldName } from "./rust-eager-service-graph.js";
import { entityUsesOptimisticConcurrency } from "@deterministic-code/generator-sdk/lib/emit-sql";
import { namesFor, layoutFor, } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
const names = namesFor({ language: "rust" });
const layout = layoutFor({ language: "rust" });
// byField path mounts at /api/<plural-kebab>/<field-kebab>/{<field_snake>}. methods are GET-only on the Rust side: the service trait exposes find_by but not update_by/delete_by, so PUT/DELETE-by-byField would require a repo-layer port and are deferred to a follow-up.
function normalizeByFields(byFields) {
    if (!Array.isArray(byFields))
        return [];
    return byFields
        .map((e) => {
        if (!e || typeof e.byField !== "string" || e.byField.length === 0)
            return null;
        const methods = Array.isArray(e.methods)
            ? e.methods.filter((m) => m === "GET")
            : ["GET"];
        if (methods.length === 0)
            return null;
        return { byField: e.byField, unique: e.byFieldUnique === true };
    })
        .filter((e) => e !== null);
}
function byFieldPrimitiveCall(entitySnake, entry, hasCoercion) {
    const coerceExpr = hasCoercion
        ? "Some(Arc::new(|row: &mut RowMap| coerce_row_types(row)))"
        : "None";
    const path = `/api/${layout.apiPath(entitySnake)}`;
    return `create_by_field_router(ByFieldRouterConfig {
        service: service.clone(),
        entity_name: "${entitySnake}".to_string(),
        base_path: "${path}".to_string(),
        field: "${entry.byField}".to_string(),
        unique: ${entry.unique ? "true" : "false"},
        methods: vec![deterministic::routes::ByFieldMethod::Get],
        coerce_row: ${coerceExpr},
        update_validator: None,
    })`;
}
function byFieldMergeChain(entitySnake, normalizedByFields, hasCoercion) {
    return normalizedByFields
        .map((e) => `    .merge(${byFieldPrimitiveCall(entitySnake, e, hasCoercion)})`)
        .join("\n");
}
function fieldTypeCheck(type) {
    switch (type) {
        case "string":
        case "uuid":
            return "is_string";
        case "number":
        case "biginteger":
        case "reference":
            return "is_number";
        case "boolean":
            return "is_boolean";
        case "datetime":
            return "is_string";
        default:
            return null;
    }
}
function buildEagerChildShapeCheck(child) {
    const name = child.fieldName;
    return `    match body.get(${JSON.stringify(name)}) {
        None => {}
        Some(v) if v.is_null() => {}
        Some(v) => match v {
            serde_json::Value::Array(arr) => {
                for (i, item) in arr.iter().enumerate() {
                    if !item.is_object() {
                        errors.push(format!("${name}[{}]: expected object", i));
                    }
                }
            }
            _ => errors.push("${name}: expected array".to_string()),
        }
    }`;
}
function requiredFieldCheck(f, requireAll) {
    const typeCheck = fieldTypeCheck(f.type);
    const present = `body.get("${f.name}").filter(|v| !v.is_null())`;
    if (requireAll) {
        if (typeCheck) {
            return `    match ${present} {
        None => errors.push("${f.name}: required".to_string()),
        Some(v) if !v.${typeCheck}() => errors.push("${f.name}: expected ${f.type}".to_string()),
        _ => {}
    }`;
        }
        return `    if ${present}.is_none() { errors.push("${f.name}: required".to_string()); }`;
    }
    if (typeCheck) {
        return `    if let Some(v) = ${present} {
        if !v.${typeCheck}() { errors.push("${f.name}: expected ${f.type}".to_string()); }
    }`;
    }
    return "";
}
function buildValidatorFn({ fnName, requiredFields, requireAll, directFkChildren, }) {
    const requiredChecks = requiredFields
        .map((f) => requiredFieldCheck(f, requireAll))
        .filter((s) => s.length > 0);
    const childShapeChecks = Array.isArray(directFkChildren)
        ? directFkChildren.map(buildEagerChildShapeCheck)
        : [];
    const checks = [...requiredChecks, ...childShapeChecks].join("\n");
    if (checks.length === 0) {
        return `fn ${fnName}(_body: &RowMap) -> Result<(), Vec<String>> { Ok(()) }`;
    }
    return `fn ${fnName}(body: &RowMap) -> Result<(), Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
${checks}
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}`;
}
function applyEnrichmentToRequiredFields(requiredFields, enrichments) {
    if (!enrichments || enrichments.length === 0)
        return requiredFields;
    const requiredNames = new Set(requiredFields.map((f) => f.name));
    const fkSet = new Set(enrichments.map((e) => e.fkColumn));
    const out = requiredFields.filter((f) => !fkSet.has(f.name));
    for (const e of enrichments) {
        // The enriched name is required only when its FK column was itself required (non-nullable); a nullable FK like an optional self-reference (role.parent_role_id) makes the enriched name optional too.
        if (requiredNames.has(e.fkColumn)) {
            out.push({
                name: e.newField,
                type: "string",
                isNullable: false,
                hasDefault: false,
            });
        }
    }
    return out;
}
function emitCoerceRowFn(booleanFields, binaryFields) {
    const boolNames = (booleanFields ?? []).map((f) => f.name);
    const binNames = (binaryFields ?? []).map((f) => f.name);
    const boolArr = boolNames.length
        ? `&[${boolNames.map((n) => JSON.stringify(n)).join(", ")}]`
        : "&[]";
    const binArr = binNames.length
        ? `&[${binNames.map((n) => JSON.stringify(n)).join(", ")}]`
        : "&[]";
    return `fn coerce_row_types(row: &mut RowMap) {
    let bool_cols: &[&str] = ${boolArr};
    let binary_cols: &[&str] = ${binArr};
    for col in bool_cols {
        if let Some(v) = row.get(*col).cloned() {
            match v {
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        row.insert((*col).to_string(), Value::Bool(i != 0));
                    }
                }
                Value::Bool(_) | Value::Null => {}
                _ => {}
            }
        }
    }
    for col in binary_cols {
        if let Some(v) = row.get(*col).cloned() {
            match v {
                Value::Array(arr) => {
                    let bytes: Vec<u8> = arr
                        .iter()
                        .filter_map(|x| x.as_u64().and_then(|n| u8::try_from(n).ok()))
                        .collect();
                    let encoded = base64_encode(&bytes);
                    row.insert((*col).to_string(), Value::String(encoded));
                }
                Value::Null | Value::String(_) => {}
                _ => {}
            }
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(CHARSET[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 12) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 6) & 0x3f) as usize] as char);
        out.push(CHARSET[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(CHARSET[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(CHARSET[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 12) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}`;
}
function partitionFieldsByType(fields) {
    const arr = Array.isArray(fields) ? fields : [];
    return {
        booleanFields: arr.filter((f) => f.type === "boolean"),
        binaryFields: arr.filter((f) => f.type === "binary"),
    };
}
function idTypeVariantForPk(primaryKey) {
    switch (primaryKey?.idType) {
        case "uuid":
            return "Uuid";
        case "string":
            return "String";
        case "integer":
        case "biginteger":
            return "Integer";
        default:
            // PKs emitted without an explicit idType (e.g. the services path) fall back to the rust type.
            return primaryKey?.rustType === "String" ? "String" : "Integer";
    }
}
function sortedDirectFkChildren(eagerWriteChildren) {
    return Array.isArray(eagerWriteChildren)
        ? eagerWriteChildren
            .filter((c) => (c.kind ?? "direct-fk") === "direct-fk")
            .slice()
            .sort((a, b) => a.fieldName.localeCompare(b.fieldName))
        : [];
}
function buildCrudRouterCtx(args) {
    const { name, requiredFields, enrichments, allFields, primaryKey, eagerWriteChildren, byFields, opts, useOptimisticConcurrency, } = args;
    const entitySnake = name;
    const className = names.className(name, "service");
    const serviceImport = serviceUseLine(name, className, opts);
    const path = `/api/${layout.apiPath(name)}`;
    const normalizedByFields = normalizeByFields(byFields);
    const hasByFields = normalizedByFields.length > 0;
    const directFkChildren = sortedDirectFkChildren(eagerWriteChildren);
    const { booleanFields, binaryFields } = partitionFieldsByType(allFields);
    const hasCoercion = booleanFields.length + binaryFields.length > 0;
    // Emitting the coercion helpers without a caller trips verify's zero-warning cargo contract (dead_code).
    const coerceFn = hasCoercion
        ? `\n\n${emitCoerceRowFn(booleanFields, binaryFields)}`
        : "";
    const pk = primaryKey ?? { column: "id", rustType: "i64" };
    const { createValidator, updateValidator } = buildCrudValidators(entitySnake, applyEnrichmentToRequiredFields(requiredFields, enrichments), directFkChildren);
    return {
        entitySnake,
        className,
        serviceImport,
        path,
        normalizedByFields,
        hasByFields,
        hasCoercion,
        opts,
        useOptimisticConcurrency: useOptimisticConcurrency === true,
        coerceFn,
        createValidator,
        updateValidator,
        idTypeVariant: idTypeVariantForPk(pk),
        primaryKeyParamExpr: pk.column === "id" ? "None" : `Some("${pk.column}".to_string())`,
        coerceRowExpr: hasCoercion
            ? "Some(Arc::new(|row: &mut RowMap| coerce_row_types(row)))"
            : "None",
    };
}
function buildCrudValidators(entitySnake, effectiveRequiredFields, children) {
    return {
        createValidator: buildValidatorFn({
            fnName: `validate_create_${entitySnake}`,
            requiredFields: effectiveRequiredFields,
            requireAll: true,
            directFkChildren: children,
        }),
        updateValidator: buildValidatorFn({
            fnName: `validate_update_${entitySnake}`,
            requiredFields: effectiveRequiredFields,
            requireAll: false,
            directFkChildren: children,
        }),
    };
}
// Always a thin create_crud_router: enrichment + eager write/read live in the composed service stack the facade forwards to, so the router keeps only validation + coercion (no enrich hooks, no lookup params).
function emitCrudRouterContent(args) {
    return emitCrudRouterPlain(buildCrudRouterCtx(args));
}
function crudRoutesImport(hasByFields) {
    return hasByFields
        ? "use deterministic::routes::{create_by_field_router, create_crud_router, ByFieldRouterConfig, CrudRouterConfig, IdType};"
        : "use deterministic::routes::{create_crud_router, CrudRouterConfig, IdType};";
}
function crudRouterConfigCall(ctx, hooks) {
    const { enrichItems, enrichItem, resolveItem } = hooks;
    const serviceExpr = ctx.hasByFields ? "service.clone()" : "service";
    return `create_crud_router(CrudRouterConfig {
        service: ${serviceExpr},
        entity_name: "${ctx.entitySnake}".to_string(),
        base_path: "${ctx.path}".to_string(),
        id_type: IdType::${ctx.idTypeVariant},
        primary_key_param: ${ctx.primaryKeyParamExpr},
        use_optimistic_concurrency: ${ctx.useOptimisticConcurrency ? "true" : "false"},
        create_validator: Some(Arc::new(|body: &RowMap| validate_create_${ctx.entitySnake}(body))),
        update_validator: Some(Arc::new(|body: &RowMap| validate_update_${ctx.entitySnake}(body))),
        patch_validator: None,
        coerce_row: ${ctx.coerceRowExpr},
        enrich_items: ${enrichItems},
        enrich_item: ${enrichItem},
        resolve_item: ${resolveItem},
    })`;
}
function withByFieldChain(ctx, coreCall) {
    if (!ctx.hasByFields)
        return coreCall;
    return `${coreCall}
${byFieldMergeChain(ctx.entitySnake, ctx.normalizedByFields, ctx.hasCoercion)}`;
}
const NO_ENRICH_HOOKS = {
    enrichItems: "None",
    enrichItem: "None",
    resolveItem: "None",
};
function emitCrudRouterPlain(ctx) {
    const primitiveImports = [
        "use std::sync::Arc;",
        "",
        ctx.serviceImport,
        crudRoutesImport(ctx.hasByFields),
        "use deterministic::RowMap;",
        ...(ctx.hasCoercion ? ["use serde_json::Value;"] : []),
    ].join("\n");
    const routerBody = withByFieldChain(ctx, crudRouterConfigCall(ctx, NO_ENRICH_HOOKS));
    return `${primitiveImports}

pub fn router(service: Arc<${ctx.className}>) -> axum::Router {
    ${routerBody}
}

${ctx.createValidator}

${ctx.updateValidator}
${ctx.coerceFn}`;
}
function emitReadOnlyRouterContent(name, byFields, opts) {
    const entitySnake = name;
    const className = names.className(name, "service");
    const serviceImport = serviceUseLine(name, className, opts);
    const path = `/api/${layout.apiPath(name)}`;
    const normalizedByFields = normalizeByFields(byFields);
    const hasByFields = normalizedByFields.length > 0;
    const idType = `IdType::${opts.idTypeVariant}`;
    if (!hasByFields) {
        return `use std::sync::Arc;

${serviceImport}
use deterministic::routes::{create_read_only_router, IdType, ReadOnlyRouterConfig};

pub fn router(service: Arc<${className}>) -> axum::Router {
    create_read_only_router(ReadOnlyRouterConfig {
        service,
        entity_name: "${entitySnake}".to_string(),
        base_path: "${path}".to_string(),
        id_type: ${idType},
        enrich_items: None,
        enrich_item: None,
    })
}
`;
    }
    const byFieldMerges = byFieldMergeChain(name, normalizedByFields, false);
    return `use std::sync::Arc;

${serviceImport}
use deterministic::routes::{create_by_field_router, create_read_only_router, ByFieldRouterConfig, IdType, ReadOnlyRouterConfig};

pub fn router(service: Arc<${className}>) -> axum::Router {
    create_read_only_router(ReadOnlyRouterConfig {
        service: service.clone(),
        entity_name: "${entitySnake}".to_string(),
        base_path: "${path}".to_string(),
        id_type: ${idType},
        enrich_items: None,
        enrich_item: None,
    })
${byFieldMerges}
}
`;
}
export function emitReadOnlyRouter(candidate, options = {}) {
    const byFeature = options.organizeByFeature === true;
    const layout = rustLayout(options);
    return {
        path: layout.filePath(candidate.name, "route"),
        content: emitReadOnlyRouterContent(candidate.name, Array.isArray(candidate.byFields) ? candidate.byFields : [], {
            byFeature,
            layout,
            idTypeVariant: idTypeVariantForPk(candidate.primaryKey),
        }),
    };
}
/** Optimistic concurrency mirrors the runtime service layer (rust/src/run/server.rs build_service_registry): m2m junctions and readonly lookups never participate (their rows carry no client-managed version), an explicit per-type `use_optimistic_concurrency` overrides the datasource-wide flag, and everything else inherits it. Router and composed service must agree per entity or the router 428s a mutation the service would allow. */
function entityUsesOcc(candidate, globalFlag) {
    return entityUsesOptimisticConcurrency(candidate, globalFlag);
}
export function emitCrudRouter(candidate, options = {}) {
    const { requiredFields = [] } = options;
    const byFeature = options.organizeByFeature === true;
    const layout = rustLayout(options);
    const allFields = Array.isArray(options.allFields) && options.allFields.length > 0
        ? options.allFields
        : Array.isArray(candidate.fields)
            ? candidate.fields
            : [];
    const enrichments = Array.isArray(candidate.enrichments)
        ? candidate.enrichments
        : [];
    const eagerWriteChildren = Array.isArray(candidate.eagerWriteChildren)
        ? candidate.eagerWriteChildren
        : [];
    return {
        path: layout.filePath(candidate.name, "route"),
        content: emitCrudRouterContent({
            name: candidate.name,
            requiredFields,
            enrichments,
            allFields,
            primaryKey: candidate.primaryKey ?? { column: "id", rustType: "i64" },
            eagerWriteChildren,
            byFields: Array.isArray(candidate.byFields) ? candidate.byFields : [],
            opts: { byFeature, layout },
            useOptimisticConcurrency: entityUsesOcc(candidate, options.useOptimisticConcurrency === true),
        }),
    };
}
// Rust serves a custom route through the spec-driven dynamic router (routes_doc.generic -> build_router in rust/src/run/server.rs), which dispatches by the route's `service` to its custom-service stub (ServiceError::Stub -> HTTP 501). A per-route file has no runtime consumer, so the Rust route stub emits nothing; the backing custom-service stub is the 501 override surface.
export function emitCustomRouteStub() {
    return null;
}
/** The generated app-wiring aggregator: `compose_router(ctx)` builds each generated service facade from
 * the ComposeContext (each pulls its composed runtime stack) and merges the generated router that routes
 * to it — the single live source of truth. The runtime's RouteComposer hook calls this. */
export function emitAppWiring(wiring, options = {}) {
    const byFeature = options.organizeByFeature === true;
    const layout = rustLayout(options);
    const opts = { byFeature, layout };
    const survivors = wiring.routers.map((r) => r.name);
    const imports = survivors.map((name) => serviceUseLine(name, names.className(name, "service"), opts));
    const lets = survivors.map((name) => `    let ${serviceFieldName(name)} = std::sync::Arc::new(${names.className(name, "service")}::from_context(ctx)?);`);
    const merges = wiring.routers.map((r) => `        .merge(${routeModulePath(r.name, opts)}::router(${serviceFieldName(r.name)}.clone()))`);
    const body = merges.length > 0
        ? `    let router = axum::Router::new()\n${merges.join("\n")};\n    Ok(router)`
        : `    Ok(axum::Router::new())`;
    const content = `use deterministic::{ComposeContext, RepositoryError};
${imports.join("\n")}

/// Builds each generated service facade from the ComposeContext and merges the generated router that
/// routes to it — the live source of truth for CRUD, enrich, and eager behavior (all from the runtime).
pub fn compose_router(ctx: &ComposeContext) -> Result<axum::Router, RepositoryError> {
${lets.join("\n")}${lets.length > 0 ? "\n\n" : ""}${body}
}
`;
    return { path: appWiringFilePath(byFeature), content };
}
/** Catalog `routes` step (rust). */
export const emit = (ctx) => routesStepEmit({
    dispatchStep: dispatchRoutesStep,
    emitter: { createEmitter },
    language: "rust",
}, ctx);
export const createEmitter = () => ({
    emit: (config) => emitRoutesFiles({
        ...config,
        primitives: {
            emitCrudRouter,
            emitReadOnlyRouter,
            emitCustomRouteStub,
            emitAppWiring,
            nestedRouterEmitters: {},
        },
    }),
});
