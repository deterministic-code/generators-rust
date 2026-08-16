/** Catalog entry for the rust `backend_app` step. The render module — driver + every Cargo/Dockerfile/dialect helper — lives at `scripts/lib/emit-backend-app-rust.ts` (its ~15 relative imports and cross-module consumers keep it there); this module is the convention filename the `emit.mjs` runner dynamic-imports, and it exports the self-describing surface over that driver. `rustModuleWiring = false`: backend_app emits `src/lib.rs` with an empty MODULES marker block that later per-module steps patch, so the generic mod.rs pass must not run. */
export declare const emit: ({ inputs, args }: import("@deterministic-code/generator-sdk/codegen/lib/backend-app-emit-helpers").BackendAppEmitContext) => Promise<{
    entries: unknown;
}>;
export declare const entriesNative = true;
export declare const pinProjectRoot = true;
export declare const flags: {
    flag: string;
    target: string;
    kind: string;
    defaultValue: boolean;
    description: string;
}[];
export declare const rustModuleWiring = false;
