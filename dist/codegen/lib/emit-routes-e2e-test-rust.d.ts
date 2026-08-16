import type { EmittedFile, RoutesEmitConfig } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit-types";
import type { DatasourceSettings } from "@deterministic-code/generator-sdk/datasource-settings";
interface FieldDef {
    type?: string;
    is_nullable?: boolean;
    default_value?: unknown;
    references?: string;
}
interface TypeDef {
    datasource_type?: string;
    fields?: Record<string, FieldDef>[];
}
interface E2ETestInput {
    datasourceData?: {
        types?: Record<string, TypeDef>[];
    };
    datasourceSettings?: DatasourceSettings;
}
export declare function emitAppE2ETest({ datasourceData, datasourceSettings, }: E2ETestInput): EmittedFile;
/** Catalog `routes_e2e_test` step (rust). */
export declare const emit: (ctx: unknown) => Promise<unknown>;
export declare const createEmitter: () => {
    emit: (config: RoutesEmitConfig & {
        expanded: unknown;
    }) => EmittedFile[];
};
export {};
