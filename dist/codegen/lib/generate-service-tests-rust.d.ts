import type { GeneratedFile, ServiceTestsGenerateConfig } from "@deterministic-code/generator-sdk/codegen/lib/service-tests-generate-types";
interface RustTestCandidate {
    name: string;
}
interface RustTestGenerateOptions {
    packageName?: string;
    organizeByFeature?: boolean;
}
export declare const DEFAULT_GENERATE_OPTIONS: {
    readonly packageName: "generated";
};
export declare function generateGenericServiceTest(_candidate: RustTestCandidate, _opts?: RustTestGenerateOptions): GeneratedFile | null;
export declare const generate: (ctx: unknown) => Promise<unknown>;
export declare const createGenerator: () => {
    generate: (config: ServiceTestsGenerateConfig) => GeneratedFile[];
};
export {};
