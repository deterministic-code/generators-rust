import type { EmittedFile } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit-types";
interface Piece {
    target: string;
    content: string;
    section?: string;
}
interface CustomService {
    struct: string;
}
/** The `mod.rs`/`lib.rs` patch pieces a rust step contributes for the source files it emits (targets are src-root-relative; the caller re-addresses them). Pure — no directory reads. */
export declare function rustModulePieces(srcRelPaths: string[], customServices?: Map<string, CustomService>): Piece[];
/** mod.rs is a crate module iff it sits under a `src/` root, outside the bundled library and the bin/ tree; src-root files (lib.rs/main.rs) and mod.rs itself are never wired. */
export declare function isWireableRustFile(srcRelPath: string): boolean;
/**
 * The `{kind:PATCH}` module-wiring entries a rust step contributes for the files
 * it just emitted (`files: {path, content}` with `path` relative to `outDir`).
 * Derives everything from that emitted list — never reads the directory. Returns
 * `[]` when the step's output isn't under a crate `src/` (backend_app's crate
 * root, migrate, perf bin, tests/).
 */
export declare function rustModuleEntriesFrom({ outDir, files, }: {
    outDir: string;
    files: EmittedFile[];
}): {
    section?: string | undefined;
    kind: string;
    filename: string;
    content: string;
}[];
export {};
