import type { DatasourceValidatorEmitConfig } from "@deterministic-code/generator-sdk/codegen/lib/datasource-validator-emit-types";
export declare const DEFAULT_EMIT_OPTIONS: {
    schemaVersion: string;
};
export declare const createEmitter: () => {
    emit: (config: DatasourceValidatorEmitConfig) => any[];
};
