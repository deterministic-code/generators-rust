import { type DatasourceOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import type { NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
type Flatten<T> = {
    [K in keyof T]: T[K];
};
type RustGenerateOptions = Flatten<NamesForOptions & DatasourceOptions & {
    schemaVersion: string;
}>;
export declare const DEFAULT_GENERATE_OPTIONS: RustGenerateOptions;
export declare function generateForTable(entry: Record<string, unknown>, datasource: unknown, options?: Partial<RustGenerateOptions>): {
    path: string;
    content: string;
};
export declare const generateFromSchema: (data: any, options: any) => any, createGenerator: () => {
    generate: (config: {
        settings: import("@deterministic-code/generator-sdk/read-settings").ParsedSettings;
        language: string;
    }) => any[];
};
export {};
