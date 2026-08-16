/** Self-describing emit for the rust datasource-type tests — wraps the shared `emit-datasource-tests-rust` render via `makeDatasourceEmit`. */
export declare const emit: ({ inputs, settings }: import("@deterministic-code/generator-sdk/codegen/lib/datasource-emit-config").DatasourceEmitContext) => Promise<{
    files: unknown;
}>;
