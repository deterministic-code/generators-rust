import type { MigrateEntry, MigrateRenderOptions } from "@deterministic-code/generator-sdk/codegen/lib/migrate-scripts-generate-types";
/** All rust migrate output as CONTENT + PATCH entries. A combined scaffold patches backend_app's Cargo.toml/Dockerfile/.env/entrypoint; a standalone scope's Cargo.toml is created by the seed patch and the sibling patches no-op on the absent files. */
declare function rustEntries({ migrateDir, dialects, settings, combined, }: MigrateRenderOptions): Promise<MigrateEntry[]>;
export declare const migrateRust: {
    language: string;
    generate: typeof rustEntries;
};
export declare const generate: ({ inputs, settings, args }: import("@deterministic-code/generator-sdk/codegen/lib/migrate-generate-helpers").MigrateGenerateArgs) => Promise<{
    entries: import("@deterministic-code/generator-sdk/codegen/lib/generate-result").GenerateEntry[];
}>;
export declare const flags: ({
    flag: string;
    target: string;
    kind: string;
    defaultValue: boolean;
    description: string;
} | {
    flag: string;
    target: string;
    kind: string;
    defaultValue: string;
    placeholder: string;
    description: string;
})[];
export declare const entriesNative = true;
export declare const pinProjectRoot = true;
export declare const rustModuleWiring = false;
export {};
