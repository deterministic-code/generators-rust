import {
  emitRoutesTestsFiles,
  dispatchRoutesTestsStep,
  routesStepEmit,
} from "@deterministic-code/generator-sdk/codegen/lib/routes-emit";
import type {
  EmittedFile,
  RoutesEmitConfig,
} from "@deterministic-code/generator-sdk/codegen/lib/routes-emit-types";

interface TestCandidate {
  name: string;
  datasourceType?: string;
}

interface RustTestOptions {
  packageName?: string;
  apiBase?: string;
  organizeByFeature?: boolean;
}

export const DEFAULT_EMIT_OPTIONS = {
  packageName: "generated",
  apiBase: "/api",
};

// Retired tier: per-entity router tests built the router via the removed `::new` API and asserted the enrich-hook router shape (routers are plain now). Emits nothing.
export function emitCrudRouterTest(
  _candidate: TestCandidate,
  _opts: RustTestOptions = DEFAULT_EMIT_OPTIONS,
): EmittedFile | null {
  return null;
}

export function emitReadOnlyRouterTest(
  _candidate: TestCandidate,
  _opts: RustTestOptions = DEFAULT_EMIT_OPTIONS,
): EmittedFile | null {
  return null;
}

/** Catalog `routes_tests` step (rust). `rustModuleWiring = false`: route tests live under `tests/`, matching create-routes-tests. */
export const emit = (ctx: unknown) =>
  routesStepEmit(
    {
      dispatchStep: dispatchRoutesTestsStep,
      emitter: { createEmitter },
      language: "rust",
    },
    ctx,
  );

export const rustModuleWiring = false;

export const createEmitter = () => ({
  emit: (config: RoutesEmitConfig) =>
    emitRoutesTestsFiles({
      ...config,
      primitives: {
        emitReadOnlyRouterTest,
        emitCrudRouterTest,
        defaultEmitOptions: DEFAULT_EMIT_OPTIONS,
      },
    }),
});
