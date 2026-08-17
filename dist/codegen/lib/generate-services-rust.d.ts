import type { ParsedSettings } from "@deterministic-code/generator-sdk/read-settings";
import type { CaseFormat } from "@deterministic-code/generator-sdk/case";
interface ServiceCandidate {
    name: string;
}
interface CustomServiceEntry {
    name: string;
}
interface RustGenerateOptions {
    organizeByFeature?: boolean;
    fieldFormat?: CaseFormat;
}
interface CustomStubOptions {
    methods?: string[];
    responseSamples?: Map<string, unknown>;
    byFeature?: boolean;
}
interface GeneratedFile {
    path: string;
    content: string;
}
interface ServicesGenerateConfig {
    services: unknown;
    viewTypes: unknown;
    datasourceTypes: unknown;
    routes: unknown;
    settings: ParsedSettings;
    language: unknown;
}
export declare function generateGenericService(candidate: ServiceCandidate, options?: RustGenerateOptions): GeneratedFile | null;
export declare function generateCustomServiceStub(entry: CustomServiceEntry, options?: CustomStubOptions): GeneratedFile;
export declare function resolveRustCustomServicePath(entry: CustomServiceEntry, byFeature?: boolean): string;
export declare function rustStructName(entryName: string): string;
export declare function rustFileBaseFor(entryName: string): string;
/** Catalog `services` step (rust). */
export declare const generate: (ctx: unknown) => Promise<unknown>;
/** Generator owns its render primitives + options; the shared orchestration in services-generate.ts does the rest. */
export declare const createGenerator: () => {
    generate: (config: ServicesGenerateConfig) => import("@deterministic-code/generator-sdk/codegen/lib/service-tests-generate-types").GeneratedFile[];
};
export {};
