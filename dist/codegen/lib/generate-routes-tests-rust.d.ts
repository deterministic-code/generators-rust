import type { GeneratedFile, RoutesGenerateConfig } from "@deterministic-code/generator-sdk/codegen/lib/routes-generate-types";
interface TestCandidate {
    name: string;
    datasourceType?: string;
}
interface RustTestOptions {
    packageName?: string;
    apiBase?: string;
    organizeByFeature?: boolean;
}
export declare const DEFAULT_GENERATE_OPTIONS: {
    packageName: string;
    apiBase: string;
};
export declare function generateCrudRouterTest(_candidate: TestCandidate, _opts?: RustTestOptions): GeneratedFile | null;
export declare function generateReadOnlyRouterTest(_candidate: TestCandidate, _opts?: RustTestOptions): GeneratedFile | null;
/** Catalog `routes_tests` step (rust). `rustModuleWiring = false`: route tests live under `tests/`, matching create-routes-tests. */
export declare const generate: (ctx: unknown) => Promise<unknown>;
export declare const rustModuleWiring = false;
export declare const createGenerator: () => {
    generate: (config: RoutesGenerateConfig) => GeneratedFile[];
};
export {};
