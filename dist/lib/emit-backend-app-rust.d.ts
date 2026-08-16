import type { EmitEntry } from "@deterministic-code/generator-sdk/codegen/lib/emit-result";
interface EmitArgs {
    input?: string;
    deterministicDir?: string;
    combined?: boolean;
    language?: string;
}
/** Lib + bin target name of every emitted consumer crate; main.rs (bin) references generated modules through it. The runtime dep keeps its natural name `deterministic`, so emitted code resolves it from the extern prelude everywhere — no extern-crate alias. */
export declare const RUST_SYNTHESIZED_CRATE_NAME = "generated";
export declare function emitMainRs({ packageName, }?: {
    packageName?: string;
}): {
    path: string;
    content: string;
};
/** Library entry point: the MODULES block starts empty (each module step patches its own `pub mod X;` line in) and custom_services() lives here — not main.rs — so cargo tests booting via build_app register the same custom services the bin does. `route_composer()` resolves the app-wiring aggregator whose module path tracks the layout (by-feature vs flat). */
export declare function emitLibRs({ byFeature }?: {
    byFeature?: boolean;
}): {
    path: string;
    content: string;
};
export declare const DEFAULT_REGISTRY_NAME = "deterministic-code";
export declare const DEFAULT_REGISTRY_INDEX_URL = "sparse+https://deterministic-code.com/registry/";
export declare function renderCargoToml({ packageName, libraryReferenceMode, crateVersion, registryName, }?: {
    packageName?: string;
    libraryReferenceMode?: string;
    crateVersion?: string | null;
    registryName?: string;
}): string;
/** why .cargo/config.toml at crate root: cargo discovers alternative registries via this file (or a global ~/.cargo/config.toml). Emitting it sibling to Cargo.toml makes the consumer crate self-contained — `cargo build` in a fresh clone of the emitted project finds the registry without any out-of-band setup. */
export declare function renderRustCargoConfig({ registryName, indexUrl, }?: {
    registryName?: string;
    indexUrl?: string;
}): string;
/** why builder-based runtime + self-diagnosing HEALTHCHECK: the runtime stage extends the rust builder (keeps cargo + sources) so verify's in-container `cargo test` runs — a debian-slim runner strips cargo; HEALTHCHECK shape mirrors the TS Dockerfile so a failed probe shows a usable status code instead of "(no output)" in docker inspect. In multi-lang the root compose sets `context: .` + `dockerfile: ./rust/Dockerfile`, so lane-relative COPY lines (Cargo.toml, src, _deterministic/.cargo, scripts) carry the `rust/` prefix while root-shared deterministic/ and the migrate-patched `COPY sql` stay reachable from the root context. */
export declare function renderRustDockerfile({ binaryName, libraryReferenceMode, multiLanguage, laneDir, }?: {
    binaryName?: string;
    libraryReferenceMode?: string;
    multiLanguage?: boolean;
    laneDir?: string;
}): string;
export declare function readLibraryCrateVersion(): Promise<string>;
export declare function emitBackendApp(args: EmitArgs): Promise<EmitEntry[]>;
export declare function buildRustCargoTomlDepsBlock(dialects?: string[]): string;
export declare function rustDialectUseImportsForSetup(dialects?: string[]): string;
export declare function rustDialectUseImportsForRunners(dialects?: string[]): string;
export declare function rustDialectSqliteUrlHelperBlock(dialects?: string[]): string;
export declare function rustDialectDdlConstsBlock(dialects?: string[]): string;
export declare function rustDialectSetupDispatchBlock(dialects?: string[]): string;
export declare function rustDialectRunnerFnsBlock(dialects?: string[]): string;
export declare function rustDialectUpDispatchBlock(dialects?: string[]): string;
export declare function rustDialectRollbackFnsBlock(dialects?: string[]): string;
export declare function rustDialectDownDispatchBlock(dialects?: string[]): string;
export {};
