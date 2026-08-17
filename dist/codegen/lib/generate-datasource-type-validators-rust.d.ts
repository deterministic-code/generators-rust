/** Self-describing generate for the rust datasource-type validators — wraps the shared `generate-datasource-validator-rust` render via `makeDatasourceGenerate`. */
export declare const generate: ({ inputs, settings }: import("@deterministic-code/generator-sdk/codegen/lib/datasource-generate-config").DatasourceGenerateContext) => Promise<{
    files: unknown;
}>;
