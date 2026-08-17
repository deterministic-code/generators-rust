import type { DatasourceValidatorGenerateConfig } from "@deterministic-code/generator-sdk/codegen/lib/datasource-validator-generate-types";
export declare const DEFAULT_GENERATE_OPTIONS: {
    schemaVersion: string;
};
export declare const createGenerator: () => {
    generate: (config: DatasourceValidatorGenerateConfig) => any[];
};
