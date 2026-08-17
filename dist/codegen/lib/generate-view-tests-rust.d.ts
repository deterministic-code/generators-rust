import type { ParsedSettings } from "@deterministic-code/generator-sdk/read-settings";
import { type DatasourceOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import { type Datasource, type View } from "@deterministic-code/generator-sdk/codegen/lib/generate-view-shared";
import type { NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
type Flatten<T> = {
    [K in keyof T]: T[K];
};
type RustGenerateOptions = Flatten<NamesForOptions & DatasourceOptions & {
    schemaVersion: string;
}>;
interface GeneratedFile {
    path: string;
    content: string;
}
export declare const DEFAULT_GENERATE_OPTIONS: RustGenerateOptions;
export declare function generateForView(view: View, { datasource, viewIndex, }: {
    datasource: Datasource;
    viewIndex: Map<string, View>;
}, options?: Partial<RustGenerateOptions>): GeneratedFile;
export declare function generateFromSchema({ viewTypes, datasource }: {
    viewTypes: unknown;
    datasource: Datasource;
}, options?: Partial<RustGenerateOptions>): GeneratedFile[];
export declare const createGenerator: () => {
    generate: (config: {
        settings: ParsedSettings;
        language: string;
    }) => any[];
};
export {};
