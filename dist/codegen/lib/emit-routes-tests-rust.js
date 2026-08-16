import { emitRoutesTestsFiles, dispatchRoutesTestsStep, routesStepEmit, } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit";
export const DEFAULT_EMIT_OPTIONS = {
    packageName: "generated",
    apiBase: "/api",
};
// Retired tier: per-entity router tests built the router via the removed `::new` API and asserted the enrich-hook router shape (routers are plain now). Emits nothing.
export function emitCrudRouterTest(_candidate, _opts = DEFAULT_EMIT_OPTIONS) {
    return null;
}
export function emitReadOnlyRouterTest(_candidate, _opts = DEFAULT_EMIT_OPTIONS) {
    return null;
}
/** Catalog `routes_tests` step (rust). `rustModuleWiring = false`: route tests live under `tests/`, matching create-routes-tests. */
export const emit = (ctx) => routesStepEmit({
    dispatchStep: dispatchRoutesTestsStep,
    emitter: { createEmitter },
    language: "rust",
}, ctx);
export const rustModuleWiring = false;
export const createEmitter = () => ({
    emit: (config) => emitRoutesTestsFiles({
        ...config,
        primitives: {
            emitReadOnlyRouterTest,
            emitCrudRouterTest,
            defaultEmitOptions: DEFAULT_EMIT_OPTIONS,
        },
    }),
});
