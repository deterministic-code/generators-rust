import { emitServiceIntegrationTestsFiles, dispatchServiceIntegrationTestsStep, servicesStepEmit, } from "@deterministic-code/generator-sdk/codegen/lib/services-emit";
export const DEFAULT_EMIT_OPTIONS = {
    servicePath: null,
    fileFormat: "Snake",
    datetime: "string",
};
// Retired tier: per-entity service integration tests built services via the removed `::new` API and drove them against a repo directly (facades need a full app/registry). Emits nothing.
export function emitGenericServiceIntegrationTest(_candidate, _opts = DEFAULT_EMIT_OPTIONS) {
    return null;
}
export const emit = (ctx) => servicesStepEmit({
    dispatchStep: dispatchServiceIntegrationTestsStep,
    emitter: { createEmitter },
    language: "rust",
}, ctx);
export const createEmitter = () => ({
    emit: (config) => emitServiceIntegrationTestsFiles({
        ...config,
        primitives: {
            emitGenericServiceIntegrationTest,
            defaultEmitOptions: DEFAULT_EMIT_OPTIONS,
        },
    }),
});
