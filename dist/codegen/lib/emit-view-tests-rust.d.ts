import type { ParsedSettings } from "@deterministic-code/generator-sdk/read-settings";
import { type DatasourceOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import { type Datasource, type View } from "@deterministic-code/generator-sdk/codegen/lib/emit-view-shared";
import type { NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
type Flatten<T> = {
    [K in keyof T]: T[K];
};
type RustEmitOptions = Flatten<NamesForOptions & DatasourceOptions & {
    schemaVersion: string;
}>;
interface EmittedFile {
    path: string;
    content: string;
}
export declare const DEFAULT_EMIT_OPTIONS: RustEmitOptions;
export declare function emitForView(view: View, { datasource, viewIndex, }: {
    datasource: Datasource;
    viewIndex: Map<string, View>;
}, options?: Partial<RustEmitOptions>): EmittedFile;
export declare function emitFromSchema({ viewTypes, datasource }: {
    viewTypes: unknown;
    datasource: Datasource;
}, options?: Partial<RustEmitOptions>): EmittedFile[];
export declare const createEmitter: () => {
    emit: (config: {
        settings: ParsedSettings;
        language: string;
    }) => any[];
};
export {};
