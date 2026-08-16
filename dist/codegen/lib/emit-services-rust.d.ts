import type { ParsedSettings } from "@deterministic-code/generator-sdk/read-settings";
import type { CaseFormat } from "@deterministic-code/generator-sdk/case";
interface ServiceCandidate {
    name: string;
}
interface CustomServiceEntry {
    name: string;
}
interface RustEmitOptions {
    organizeByFeature?: boolean;
    fieldFormat?: CaseFormat;
}
interface CustomStubOptions {
    methods?: string[];
    responseSamples?: Map<string, unknown>;
    byFeature?: boolean;
}
interface EmittedFile {
    path: string;
    content: string;
}
interface ServicesEmitConfig {
    services: unknown;
    viewTypes: unknown;
    datasourceTypes: unknown;
    routes: unknown;
    settings: ParsedSettings;
    language: unknown;
}
export declare function emitGenericService(candidate: ServiceCandidate, options?: RustEmitOptions): EmittedFile | null;
export declare function emitCustomServiceStub(entry: CustomServiceEntry, options?: CustomStubOptions): EmittedFile;
export declare function resolveRustCustomServicePath(entry: CustomServiceEntry, byFeature?: boolean): string;
export declare function rustStructName(entryName: string): string;
export declare function rustFileBaseFor(entryName: string): string;
/** Catalog `services` step (rust). */
export declare const emit: (ctx: unknown) => Promise<unknown>;
/** Emitter owns its render primitives + options; the shared orchestration in services-emit.ts does the rest. */
export declare const createEmitter: () => {
    emit: (config: ServicesEmitConfig) => import("@deterministic-code/generator-sdk/codegen/lib/service-tests-emit-types").EmittedFile[];
};
export {};
