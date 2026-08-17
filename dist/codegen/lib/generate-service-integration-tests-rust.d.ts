import type { GeneratedFile, ServiceTestsGenerateConfig } from "@deterministic-code/generator-sdk/codegen/lib/service-tests-generate-types";
import type { IntegrationTestCandidate } from "@deterministic-code/generator-sdk/codegen/lib/service-integration-tests-generate-types";
interface RustGenerateOptions {
    servicePath?: string | null;
    fileFormat?: string;
    datetime?: string;
}
export declare const DEFAULT_GENERATE_OPTIONS: {
    readonly servicePath: null;
    readonly fileFormat: "Snake";
    readonly datetime: "string";
};
export declare function generateGenericServiceIntegrationTest(_candidate: IntegrationTestCandidate, _opts?: RustGenerateOptions): GeneratedFile | null;
export declare const generate: (ctx: unknown) => Promise<unknown>;
export declare const createGenerator: () => {
    generate: (config: ServiceTestsGenerateConfig) => GeneratedFile[];
};
export {};
