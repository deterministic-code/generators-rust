import type { OpenApiDocument } from "@deterministic-code/generator-sdk/codegen/lib/openapi-types";
import type { EmittedFile } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit-types";
interface SpecInfo {
    title?: string;
    version?: string;
    [key: string]: unknown;
}
interface BuiltSpec {
    info?: SpecInfo;
    [key: string]: unknown;
}
interface OpenApiDocOptions {
    title?: string;
    version?: string;
    entities?: string[];
    enrichedSpec?: OpenApiDocument | null;
}
interface ConformanceTestOptions {
    crateName?: string;
    entities?: string[];
    enrichedSpec?: OpenApiDocument | null;
}
export declare function buildOpenApiSpec({ title, version, entities, enrichedSpec, }?: OpenApiDocOptions): BuiltSpec;
export declare function emitOpenApiConformanceTest(opts?: ConformanceTestOptions): EmittedFile;
export declare function emitOpenApiRouter({ title, version, entities, enrichedSpec, }?: OpenApiDocOptions): EmittedFile;
export {};
