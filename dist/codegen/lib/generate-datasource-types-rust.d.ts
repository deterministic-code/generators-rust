import type { GeneratedFile } from "@deterministic-code/generator-sdk/codegen/lib/datasource-types-generate-types";
interface RustGenerateOptions {
    baseClass: null;
    schemaVersion: string;
    style: unknown;
    idType?: string;
    datetime?: string;
    withUuidColumn?: boolean;
}
export declare const DEFAULT_GENERATE_OPTIONS: RustGenerateOptions;
export declare const mapRustType: (dsType: string, { datetime }?: {
    datetime?: string;
}) => string;
/** The rust type for an id column. Delegates the `settings.datasource.id_type` cases to the shared `DatasourceSettings` owner; the leading guard passes a raw `i32` rust type through (the SDK owner only knows settings id_types, not rust primitives). */
export declare function normalizeIdType(idType: string | undefined): string;
export declare const render: (config: import("@deterministic-code/generator-sdk/codegen/lib/datasource-types-generate-types").DatasourceTypesGenerateConfig) => GeneratedFile[], createGenerator: () => {
    generate: (config: import("@deterministic-code/generator-sdk/codegen/lib/datasource-types-generate-types").DatasourceTypesGenerateConfig) => GeneratedFile[];
}, generate: ({ inputs, settings }: import("@deterministic-code/generator-sdk/codegen/lib/generate-settings-options").DatasourceTypesGenerateInput) => Promise<{
    files: GeneratedFile[];
}>;
export {};
