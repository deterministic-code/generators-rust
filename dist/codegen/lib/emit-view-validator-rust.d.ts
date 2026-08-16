export declare const DEFAULT_EMIT_OPTIONS: {
    schemaVersion: string;
};
/** Emitter owns its options: DEFAULT_EMIT_OPTIONS + datetime from settings; casing from CodegenNames; nested-validator paths via RustImports. */
export declare const createEmitter: () => import("@deterministic-code/generator-sdk/codegen/lib/emit-view-shared").ViewEmitter<import("@deterministic-code/generator-sdk/read-settings").ParsedSettings>;
