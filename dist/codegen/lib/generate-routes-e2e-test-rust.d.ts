import type { GeneratedFile, RoutesGenerateConfig } from "@deterministic-code/generator-sdk/codegen/lib/routes-generate-types";
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
export declare function generateAppE2ETest({ datasourceData, datasourceSettings, }: E2ETestInput): GeneratedFile;
/** Catalog `routes_e2e_test` step (rust). */
export declare const generate: (ctx: unknown) => Promise<unknown>;
export declare const createGenerator: () => {
    generate: (config: RoutesGenerateConfig & {
        expanded: unknown;
    }) => GeneratedFile[];
};
export {};
