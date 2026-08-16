import type {
  EmittedFile,
  ServiceTestsEmitConfig,
} from "@deterministic-code/generator-sdk/codegen/lib/service-tests-emit-types";
import type { IntegrationTestCandidate } from "@deterministic-code/generator-sdk/codegen/lib/service-integration-tests-emit-types";
import {
  emitServiceIntegrationTestsFiles,
  dispatchServiceIntegrationTestsStep,
  servicesStepEmit,
} from "@deterministic-code/generator-sdk/codegen/lib/services-emit";

interface RustEmitOptions {
  servicePath?: string | null;
  fileFormat?: string;
  datetime?: string;
}

export const DEFAULT_EMIT_OPTIONS = {
  servicePath: null,
  fileFormat: "Snake",
  datetime: "string",
} as const;

// Retired tier: per-entity service integration tests built services via the removed `::new` API and drove them against a repo directly (facades need a full app/registry). Emits nothing.
export function emitGenericServiceIntegrationTest(
  _candidate: IntegrationTestCandidate,
  _opts: RustEmitOptions = DEFAULT_EMIT_OPTIONS,
): EmittedFile | null {
  return null;
}

export const emit = (ctx: unknown) =>
  servicesStepEmit(
    {
      dispatchStep: dispatchServiceIntegrationTestsStep,
      emitter: { createEmitter },
      language: "rust",
    },
    ctx,
  );

export const createEmitter = () => ({
  emit: (config: ServiceTestsEmitConfig) =>
    emitServiceIntegrationTestsFiles({
      ...config,
      primitives: {
        emitGenericServiceIntegrationTest,
        defaultEmitOptions: DEFAULT_EMIT_OPTIONS,
      },
    }),
});
