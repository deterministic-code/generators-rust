import { type NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
import type { CodegenLayout } from "@deterministic-code/generator-sdk/codegen-layout";
interface RustUseOpts {
    byFeature: boolean;
    layout: CodegenLayout;
    root?: string;
}
/** The Rust `CodegenLayout` for a resolved emit-options object — placement (feature dirs, file paths) stays SDK-owned. */
export declare function rustLayout(options?: NamesForOptions): CodegenLayout;
/** `use <root>::…::<service-module>::<symbol>;` for a generated service a router, sibling service, or test depends on. The module stem is the SDK-canonical service file base for the entity. */
export declare function serviceUseLine(entity: string, symbol: string, opts: RustUseOpts): string;
/** `<root>::…::<routeModule>` — the entity's generated route module path (no trailing `::router`), for the app-wiring aggregator that calls each `router()` by absolute path to avoid glob collisions. */
export declare function routeModulePath(entity: string, opts: RustUseOpts): string;
/** The app-wiring aggregator is cross-entity, so it can't key off the per-entity layout. Under by-feature it lives at `features/app_wiring.rs` (wired through `features/mod.rs`); flat leaves the stem bare and the routes step prefixes it into `routes/generated/`. A src-root file would never get a `pub mod` and orphan the composer. */
export declare function appWiringFilePath(byFeature: boolean): string;
/** The `crate::…::compose_router` path matching `appWiringFilePath` — lib.rs's `route_composer()` resolves the aggregator through it. */
export declare function appWiringComposePath(byFeature: boolean): string;
export {};
