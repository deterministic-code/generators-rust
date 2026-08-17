import { generateRoutesTestsFiles, dispatchRoutesTestsStep, routesStepGenerate, } from "@deterministic-code/generator-sdk/codegen/lib/routes-generate";
export const DEFAULT_GENERATE_OPTIONS = {
    packageName: "generated",
    apiBase: "/api",
};
// Retired tier: per-entity router tests built the router via the removed `::new` API and asserted the enrich-hook router shape (routers are plain now). Generates nothing.
export function generateCrudRouterTest(_candidate, _opts = DEFAULT_GENERATE_OPTIONS) {
    return null;
}
export function generateReadOnlyRouterTest(_candidate, _opts = DEFAULT_GENERATE_OPTIONS) {
    return null;
}
/** Catalog `routes_tests` step (rust). `rustModuleWiring = false`: route tests live under `tests/`, matching create-routes-tests. */
export const generate = (ctx) => routesStepGenerate({
    dispatchStep: dispatchRoutesTestsStep,
    generator: { createGenerator },
    language: "rust",
}, ctx);
export const rustModuleWiring = false;
export const createGenerator = () => ({
    generate: (config) => generateRoutesTestsFiles({
        ...config,
        primitives: {
            generateReadOnlyRouterTest,
            generateCrudRouterTest,
            defaultGenerateOptions: DEFAULT_GENERATE_OPTIONS,
        },
    }),
});
