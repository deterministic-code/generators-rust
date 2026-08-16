import { type DatasourceOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import type { NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
type Flatten<T> = {
    [K in keyof T]: T[K];
};
type RustEmitOptions = Flatten<NamesForOptions & DatasourceOptions & {
    schemaVersion: string;
}>;
export declare const DEFAULT_EMIT_OPTIONS: RustEmitOptions;
export declare function emitForTable(entry: Record<string, unknown>, datasource: unknown, options?: Partial<RustEmitOptions>): {
    path: string;
    content: string;
};
export declare const emitFromSchema: (data: any, options: any) => any, createEmitter: () => {
    emit: (config: {
        settings: import("@deterministic-code/generator-sdk/read-settings").ParsedSettings;
        language: string;
    }) => any[];
};
export {};
