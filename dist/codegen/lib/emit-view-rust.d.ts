export declare const DEFAULT_EMIT_OPTIONS: {
    baseClass: null;
    schemaVersion: string;
    style: "none" | "simple" | "description";
};
/** Emitter owns its options: DEFAULT_EMIT_OPTIONS + datasource repr from settings; casing from CodegenNames; imports via RustImports. */
export declare const createEmitter: () => import("@deterministic-code/generator-sdk/codegen/lib/emit-view-shared").ViewEmitter<import("@deterministic-code/generator-sdk/read-settings").ParsedSettings>;
