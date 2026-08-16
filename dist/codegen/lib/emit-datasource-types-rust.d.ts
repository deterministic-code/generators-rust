import type { EmittedFile } from "@deterministic-code/generator-sdk/codegen/lib/datasource-types-emit-types";
interface RustEmitOptions {
    baseClass: null;
    schemaVersion: string;
    style: unknown;
    idType?: string;
    datetime?: string;
    withUuidColumn?: boolean;
}
export declare const DEFAULT_EMIT_OPTIONS: RustEmitOptions;
export declare const mapRustType: (dsType: string, { datetime }?: {
    datetime?: string;
}) => string;
/** The rust type for an id column. Delegates the `settings.datasource.id_type` cases to the shared `DatasourceSettings` owner; the leading guard passes a raw `i32` rust type through (the SDK owner only knows settings id_types, not rust primitives). */
export declare function normalizeIdType(idType: string | undefined): string;
export declare const render: (config: import("@deterministic-code/generator-sdk/codegen/lib/datasource-types-emit-types").DatasourceTypesEmitConfig) => EmittedFile[], createEmitter: () => {
    emit: (config: import("@deterministic-code/generator-sdk/codegen/lib/datasource-types-emit-types").DatasourceTypesEmitConfig) => EmittedFile[];
}, emit: ({ inputs, settings }: import("@deterministic-code/generator-sdk/codegen/lib/emit-settings-options").DatasourceTypesEmitInput) => Promise<{
    files: EmittedFile[];
}>;
export {};
