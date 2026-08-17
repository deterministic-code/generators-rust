import type {
  GeneratedFile,
  ServiceTestsGenerateConfig,
} from "@deterministic-code/generator-sdk/codegen/lib/service-tests-generate-types";
import type { IntegrationTestCandidate } from "@deterministic-code/generator-sdk/codegen/lib/service-integration-tests-generate-types";
import {
  generateServiceIntegrationTestsFiles,
  dispatchServiceIntegrationTestsStep,
  servicesStepGenerate,
} from "@deterministic-code/generator-sdk/codegen/lib/services-generate";

interface RustGenerateOptions {
  servicePath?: string | null;
  fileFormat?: string;
  datetime?: string;
}

export const DEFAULT_GENERATE_OPTIONS = {
  servicePath: null,
  fileFormat: "Snake",
  datetime: "string",
} as const;

// Retired tier: per-entity service integration tests built services via the removed `::new` API and drove them against a repo directly (facades need a full app/registry). Generates nothing.
export function generateGenericServiceIntegrationTest(
  _candidate: IntegrationTestCandidate,
  _opts: RustGenerateOptions = DEFAULT_GENERATE_OPTIONS,
): GeneratedFile | null {
  return null;
}

export const generate = (ctx: unknown) =>
  servicesStepGenerate(
    {
      dispatchStep: dispatchServiceIntegrationTestsStep,
      generator: { createGenerator },
      language: "rust",
    },
    ctx,
  );

export const createGenerator = () => ({
  generate: (config: ServiceTestsGenerateConfig) =>
    generateServiceIntegrationTestsFiles({
      ...config,
      primitives: {
        generateGenericServiceIntegrationTest,
        defaultGenerateOptions: DEFAULT_GENERATE_OPTIONS,
      },
    }),
});
