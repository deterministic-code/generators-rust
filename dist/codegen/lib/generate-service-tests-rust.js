import { generateServiceTestsFiles, dispatchServiceTestsStep, servicesStepGenerate, } from "@deterministic-code/generator-sdk/codegen/lib/services-generate";
export const DEFAULT_GENERATE_OPTIONS = {
    packageName: "generated",
};
// Retired tier: per-entity service unit tests built services via the removed inline `::new` API; the live path is now the runtime decorators/routes::adapt self-tests. Generates nothing.
export function generateGenericServiceTest(_candidate, _opts = DEFAULT_GENERATE_OPTIONS) {
    return null;
}
export const generate = (ctx) => servicesStepGenerate({
    dispatchStep: dispatchServiceTestsStep,
    generator: { createGenerator },
    language: "rust",
}, ctx);
export const createGenerator = () => ({
    generate: (config) => generateServiceTestsFiles({
        ...config,
        primitives: {
            generateGenericServiceTest,
            defaultGenerateOptions: DEFAULT_GENERATE_OPTIONS,
        },
    }),
});
