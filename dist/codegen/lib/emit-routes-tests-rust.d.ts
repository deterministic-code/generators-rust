import type { EmittedFile, RoutesEmitConfig } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit-types";
interface TestCandidate {
    name: string;
    datasourceType?: string;
}
interface RustTestOptions {
    packageName?: string;
    apiBase?: string;
    organizeByFeature?: boolean;
}
export declare const DEFAULT_EMIT_OPTIONS: {
    packageName: string;
    apiBase: string;
};
export declare function emitCrudRouterTest(_candidate: TestCandidate, _opts?: RustTestOptions): EmittedFile | null;
export declare function emitReadOnlyRouterTest(_candidate: TestCandidate, _opts?: RustTestOptions): EmittedFile | null;
/** Catalog `routes_tests` step (rust). `rustModuleWiring = false`: route tests live under `tests/`, matching create-routes-tests. */
export declare const emit: (ctx: unknown) => Promise<unknown>;
export declare const rustModuleWiring = false;
export declare const createEmitter: () => {
    emit: (config: RoutesEmitConfig) => EmittedFile[];
};
export {};
