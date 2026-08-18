import {
  generateServiceTestsFiles,
  dispatchServiceTestsStep,
  servicesStepGenerate,
  type GeneratedFile,
  type ServiceTestsGenerateConfig,
} from "@deterministic-code/generator-sdk/codegen/lib/services-generate";

interface RustTestCandidate {
  name: string;
}

interface RustTestGenerateOptions {
  packageName?: string;
  organizeByFeature?: boolean;
}

export const DEFAULT_GENERATE_OPTIONS = {
  packageName: "generated",
} as const;

// Retired tier: per-entity service unit tests built services via the removed inline `::new` API; the live path is now the runtime decorators/routes::adapt self-tests. Generates nothing.
export function generateGenericServiceTest(
  _candidate: RustTestCandidate,
  _opts: RustTestGenerateOptions = DEFAULT_GENERATE_OPTIONS,
): GeneratedFile | null {
  return null;
}

export const generate = (ctx: unknown) =>
  servicesStepGenerate(
    {
      dispatchStep: dispatchServiceTestsStep,
      generator: { createGenerator },
      language: "rust",
    },
    ctx,
  );

export const createGenerator = () => ({
  generate: (config: ServiceTestsGenerateConfig) =>
    generateServiceTestsFiles({
      ...config,
      primitives: {
        generateGenericServiceTest,
        defaultGenerateOptions: DEFAULT_GENERATE_OPTIONS,
      },
    }),
});
