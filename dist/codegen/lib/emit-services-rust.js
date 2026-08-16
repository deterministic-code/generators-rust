import { emitServicesFiles, dispatchServicesStep, servicesStepEmit, } from "@deterministic-code/generator-sdk/codegen/lib/services-emit";
import { snakeToPascal, camelToSnake, } from "@deterministic-code/generator-sdk/case";
import { ImportPaths } from "@deterministic-code/generator-sdk/import-paths";
import { rustLayout } from "./rust-crate-paths.js";
const STATUS_OK_DEFAULTS = {
    HealthCheckService: {
        check: `Ok(json!({ "status": "ok" }))`,
    },
};
function rustFileBase(name) {
    return camelToSnake(name); // lint-emitter-casing-allow: camelToSnake
}
function structNameFor(entryName) {
    if (/^[A-Z]/.test(entryName) && !entryName.includes("_"))
        return entryName;
    return snakeToPascal(entryName); // lint-emitter-casing-allow: snakeToPascal
}
// An un-implemented service method returns Ok(200) with a schema-valid success sample of the route's declared response — identical to the TS/C# stubs, so all three languages behave the same for a consumer. responseSample (a JS value sampled from the OpenAPI response schema) is serialized via JSON.stringify into a serde_json::json!(...) literal when present; absent (a route with no declared response) it returns Ok(json!({})). No 501 signal and no stderr trace — parity with TS/C#, which return a plain success body.
function defaultBodyFor(serviceName, method, responseSample) {
    const known = STATUS_OK_DEFAULTS[serviceName]?.[method];
    if (known)
        return known;
    const bodyLiteral = responseSample !== undefined ? JSON.stringify(responseSample) : "{}";
    return `Ok(serde_json::json!(${bodyLiteral}))`;
}
function emitMethodImpl(serviceName, method, responseSample) {
    const body = defaultBodyFor(serviceName, method, responseSample);
    const rustFn = camelToSnake(method); // lint-emitter-casing-allow: camelToSnake
    return `    async fn ${rustFn}(&self, _args: Value) -> Result<Value, ServiceError> {\n        ${body}\n    }`;
}
function emitInvokeArm(method) {
    // lint-emitter-casing-allow: camelToSnake
    return `            "${method}" => self.${camelToSnake(method)}(args).await,`;
}
// A generated service is a thin typed facade: from_context pulls the composed runtime stack from the registry by entity name, and the router-service traits forward through `deterministic::routes::adapt` — so CRUD/enrich/eager/OCC stay the runtime's tested code.
function renderServiceFacade(structName, entitySnake) {
    return `use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use deterministic::repositories::RowMap;
use deterministic::routes::adapt;
use deterministic::services::{DynamicService, ServiceError};
use deterministic::{ComposeContext, RepositoryError};

pub struct ${structName} {
    inner: Arc<dyn DynamicService>,
}

impl ${structName} {
    pub fn from_context(ctx: &ComposeContext) -> Result<Self, RepositoryError> {
        Ok(Self {
            inner: ctx.entity_service(${JSON.stringify(entitySnake)})?,
        })
    }
}

#[async_trait]
impl DynamicService for ${structName} {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
        self.inner.invoke(method, args).await
    }
}

#[async_trait]
impl deterministic::routes::CrudService for ${structName} {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        adapt::find_all(self.inner.as_ref()).await
    }
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        adapt::find(self.inner.as_ref(), id).await
    }
    async fn add(&self, body: RowMap) -> Result<RowMap, RepositoryError> {
        adapt::add(self.inner.as_ref(), body).await
    }
    async fn update(
        &self,
        id: &Value,
        body: RowMap,
        expected_updated: Option<&str>,
    ) -> Result<Option<RowMap>, RepositoryError> {
        adapt::update(self.inner.as_ref(), id, body, expected_updated).await
    }
    async fn delete(
        &self,
        id: &Value,
        expected_updated: Option<&str>,
    ) -> Result<bool, RepositoryError> {
        adapt::delete(self.inner.as_ref(), id, expected_updated).await
    }
}

#[async_trait]
impl deterministic::routes::ReadOnlyService for ${structName} {
    async fn find_all(&self) -> Result<Vec<RowMap>, RepositoryError> {
        adapt::find_all(self.inner.as_ref()).await
    }
    async fn find(&self, id: &Value) -> Result<Option<RowMap>, RepositoryError> {
        adapt::find(self.inner.as_ref(), id).await
    }
}

#[async_trait]
impl deterministic::routes::ByFieldService for ${structName} {
    async fn find_by(
        &self,
        field: &str,
        value: &Value,
    ) -> Result<Vec<RowMap>, RepositoryError> {
        adapt::find_by(self.inner.as_ref(), field, value).await
    }
}
`;
}
export function emitGenericService(candidate, options = {}) {
    if (!candidate || typeof candidate.name !== "string") {
        throw new Error("emit-services-rust.emitGenericService: candidate.name (snake_case entity) is required");
    }
    const entitySnake = candidate.name;
    const layout = rustLayout(options);
    const structName = layout.names.className(entitySnake, "service");
    return {
        path: layout.filePath(entitySnake, "service"),
        content: renderServiceFacade(structName, entitySnake),
    };
}
function renderStubInvokeBody(struct, methods) {
    if (methods.length === 0) {
        return `        Err(ServiceError::UnknownMethod(format!(\n            "${struct}.{}",\n            method\n        )))`;
    }
    return [
        `        match method {`,
        ...methods.map(emitInvokeArm),
        `            _ => Err(ServiceError::UnknownMethod(format!(`,
        `                "${struct}.{}",`,
        `                method`,
        `            ))),`,
        `        }`,
    ].join("\n");
}
// why this resolves to import-only-Value for stubs: stub bodies are emitted as fully-qualified `serde_json::json!(...)` so the bare `json` macro isn't in scope; only STATUS_OK_DEFAULTS-driven bodies (HealthCheckService.check) use `Ok(json!({...}))` and need the un-namespaced `json` macro brought in. Falling through to `use serde_json::Value;` for plain-stub services drops the unused_imports warning that was halting cargo_build_host.
function stubImports(struct) {
    return [
        "use async_trait::async_trait;",
        hasDefaultBody(struct)
            ? "use serde_json::{json, Value};"
            : "use serde_json::Value;",
        "",
        "use deterministic::services::{DynamicService, ServiceError};",
    ].join("\n");
}
export function emitCustomServiceStub(entry, options = {}) {
    const methods = Array.isArray(options.methods) ? [...options.methods] : [];
    const responseSamples = options.responseSamples instanceof Map ? options.responseSamples : null;
    const struct = structNameFor(entry.name);
    const methodImpls = methods
        .map((m) => emitMethodImpl(struct, m, responseSamples?.get(m)))
        .join("\n\n");
    const invokeBody = renderStubInvokeBody(struct, methods);
    const implBlock = methods.length === 0
        ? `impl ${struct} {\n    pub fn new() -> Self {\n        Self\n    }\n}`
        : `impl ${struct} {\n    pub fn new() -> Self {\n        Self\n    }\n\n${methodImpls}\n}`;
    const content = `${stubImports(struct)}\n\n` +
        `pub struct ${struct};\n\n` +
        `${implBlock}\n\n` +
        `impl Default for ${struct} {\n    fn default() -> Self {\n        Self::new()\n    }\n}\n\n` +
        `#[async_trait]\n` +
        `impl DynamicService for ${struct} {\n` +
        `    async fn invoke(&self, method: &str, ${methods.length === 0 ? "_args" : "args"}: Value) -> Result<Value, ServiceError> {\n` +
        `${invokeBody}\n` +
        `    }\n` +
        `}\n`;
    return {
        path: resolveRustCustomServicePath(entry, options.byFeature),
        content,
    };
}
function hasDefaultBody(serviceName) {
    return Object.prototype.hasOwnProperty.call(STATUS_OK_DEFAULTS, serviceName);
}
export function resolveRustCustomServicePath(entry, byFeature = false) {
    const fileBase = rustFileBaseFor(entry.name);
    if (byFeature) {
        return ImportPaths.customStubPath({
            className: entry.name,
            fileBase,
            ext: ".rs",
            snakeDir: true,
        });
    }
    return `../custom/${fileBase}.rs`;
}
export function rustStructName(entryName) {
    return structNameFor(entryName);
}
export function rustFileBaseFor(entryName) {
    return rustFileBase(structNameFor(entryName));
}
/** Catalog `services` step (rust). */
export const emit = (ctx) => servicesStepEmit({
    dispatchStep: dispatchServicesStep,
    emitter: { createEmitter },
    language: "rust",
}, ctx);
/** Emitter owns its render primitives + options; the shared orchestration in services-emit.ts does the rest. */
export const createEmitter = () => ({
    emit: (config) => emitServicesFiles({
        ...config,
        primitives: { emitGenericService, emitCustomServiceStub },
    }),
});
