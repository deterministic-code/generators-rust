/** Self-describing emit for the rust datasource-type validators — wraps the shared `emit-datasource-validator-rust` render via `makeDatasourceEmit`. */
export declare const emit: ({ inputs, settings }: import("@deterministic-code/generator-sdk/codegen/lib/datasource-emit-config").DatasourceEmitContext) => Promise<{
    files: unknown;
}>;
