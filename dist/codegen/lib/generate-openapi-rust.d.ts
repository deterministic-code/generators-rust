import type { OpenApiDocument } from "@deterministic-code/generator-sdk/codegen/lib/openapi-types";
import type { GeneratedFile } from "@deterministic-code/generator-sdk/codegen/lib/routes-generate-types";
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
export declare function generateOpenApiConformanceTest(opts?: ConformanceTestOptions): GeneratedFile;
export declare function generateOpenApiRouter({ title, version, entities, enrichedSpec, }?: OpenApiDocOptions): GeneratedFile;
export {};
