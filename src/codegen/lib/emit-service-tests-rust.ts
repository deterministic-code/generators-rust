import type {
  EmittedFile,
  ServiceTestsEmitConfig,
} from "@deterministic-code/generator-sdk/codegen/lib/service-tests-emit-types";
import {
  emitServiceTestsFiles,
  dispatchServiceTestsStep,
  servicesStepEmit,
} from "@deterministic-code/generator-sdk/codegen/lib/services-emit";

interface RustTestCandidate {
  name: string;
}

interface RustTestEmitOptions {
  packageName?: string;
  organizeByFeature?: boolean;
}

export const DEFAULT_EMIT_OPTIONS = {
  packageName: "generated",
} as const;

// Retired tier: per-entity service unit tests built services via the removed inline `::new` API; the live path is now the runtime decorators/routes::adapt self-tests. Emits nothing.
export function emitGenericServiceTest(
  _candidate: RustTestCandidate,
  _opts: RustTestEmitOptions = DEFAULT_EMIT_OPTIONS,
): EmittedFile | null {
  return null;
}

export const emit = (ctx: unknown) =>
  servicesStepEmit(
    {
      dispatchStep: dispatchServiceTestsStep,
      emitter: { createEmitter },
      language: "rust",
    },
    ctx,
  );

export const createEmitter = () => ({
  emit: (config: ServiceTestsEmitConfig) =>
    emitServiceTestsFiles({
      ...config,
      primitives: {
        emitGenericServiceTest,
        defaultEmitOptions: DEFAULT_EMIT_OPTIONS,
      },
    }),
});
