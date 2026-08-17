import {
  generateRoutesTestsFiles,
  dispatchRoutesTestsStep,
  routesStepGenerate,
} from "@deterministic-code/generator-sdk/codegen/lib/routes-generate";
import type {
  GeneratedFile,
  RoutesGenerateConfig,
} from "@deterministic-code/generator-sdk/codegen/lib/routes-generate-types";

interface TestCandidate {
  name: string;
  datasourceType?: string;
}

interface RustTestOptions {
  packageName?: string;
  apiBase?: string;
  organizeByFeature?: boolean;
}

export const DEFAULT_GENERATE_OPTIONS = {
  packageName: "generated",
  apiBase: "/api",
};

// Retired tier: per-entity router tests built the router via the removed `::new` API and asserted the enrich-hook router shape (routers are plain now). Generates nothing.
export function generateCrudRouterTest(
  _candidate: TestCandidate,
  _opts: RustTestOptions = DEFAULT_GENERATE_OPTIONS,
): GeneratedFile | null {
  return null;
}

export function generateReadOnlyRouterTest(
  _candidate: TestCandidate,
  _opts: RustTestOptions = DEFAULT_GENERATE_OPTIONS,
): GeneratedFile | null {
  return null;
}

/** Catalog `routes_tests` step (rust). `rustModuleWiring = false`: route tests live under `tests/`, matching create-routes-tests. */
export const generate = (ctx: unknown) =>
  routesStepGenerate(
    {
      dispatchStep: dispatchRoutesTestsStep,
      generator: { createGenerator },
      language: "rust",
    },
    ctx,
  );

export const rustModuleWiring = false;

export const createGenerator = () => ({
  generate: (config: RoutesGenerateConfig) =>
    generateRoutesTestsFiles({
      ...config,
      primitives: {
        generateReadOnlyRouterTest,
        generateCrudRouterTest,
        defaultGenerateOptions: DEFAULT_GENERATE_OPTIONS,
      },
    }),
});
