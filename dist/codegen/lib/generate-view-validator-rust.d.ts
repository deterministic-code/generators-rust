export declare const DEFAULT_GENERATE_OPTIONS: {
    schemaVersion: string;
};
/** Generator owns its options: DEFAULT_GENERATE_OPTIONS + datetime from settings; casing from CodegenNames; nested-validator paths via RustImports. */
export declare const createGenerator: () => import("@deterministic-code/generator-sdk/codegen/lib/generate-view-shared").ViewGenerator<import("@deterministic-code/generator-sdk/read-settings").ParsedSettings>;
