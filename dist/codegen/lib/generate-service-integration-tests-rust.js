import { generateServiceIntegrationTestsFiles, dispatchServiceIntegrationTestsStep, servicesStepGenerate, } from "@deterministic-code/generator-sdk/codegen/lib/services-generate";
export const DEFAULT_GENERATE_OPTIONS = {
    servicePath: null,
    fileFormat: "Snake",
    datetime: "string",
};
// Retired tier: per-entity service integration tests built services via the removed `::new` API and drove them against a repo directly (facades need a full app/registry). Generates nothing.
export function generateGenericServiceIntegrationTest(_candidate, _opts = DEFAULT_GENERATE_OPTIONS) {
    return null;
}
export const generate = (ctx) => servicesStepGenerate({
    dispatchStep: dispatchServiceIntegrationTestsStep,
    generator: { createGenerator },
    language: "rust",
}, ctx);
export const createGenerator = () => ({
    generate: (config) => generateServiceIntegrationTestsFiles({
        ...config,
        primitives: {
            generateGenericServiceIntegrationTest,
            defaultGenerateOptions: DEFAULT_GENERATE_OPTIONS,
        },
    }),
});
