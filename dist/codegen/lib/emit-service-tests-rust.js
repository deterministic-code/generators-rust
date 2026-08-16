import { emitServiceTestsFiles, dispatchServiceTestsStep, servicesStepEmit, } from "@deterministic-code/generator-sdk/codegen/lib/services-emit";
export const DEFAULT_EMIT_OPTIONS = {
    packageName: "generated",
};
// Retired tier: per-entity service unit tests built services via the removed inline `::new` API; the live path is now the runtime decorators/routes::adapt self-tests. Emits nothing.
export function emitGenericServiceTest(_candidate, _opts = DEFAULT_EMIT_OPTIONS) {
    return null;
}
export const emit = (ctx) => servicesStepEmit({
    dispatchStep: dispatchServiceTestsStep,
    emitter: { createEmitter },
    language: "rust",
}, ctx);
export const createEmitter = () => ({
    emit: (config) => emitServiceTestsFiles({
        ...config,
        primitives: {
            emitGenericServiceTest,
            defaultEmitOptions: DEFAULT_EMIT_OPTIONS,
        },
    }),
});
