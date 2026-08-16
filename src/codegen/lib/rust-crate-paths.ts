import {
  layoutFor,
  type NamesForOptions,
} from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
import type { CodegenLayout } from "@deterministic-code/generator-sdk/codegen-layout";

interface RustUseOpts {
  byFeature: boolean;
  layout: CodegenLayout;
  root?: string;
}

/** The Rust `CodegenLayout` for a resolved emit-options object — placement (feature dirs, file paths) stays SDK-owned. */
export function rustLayout(options: NamesForOptions = {}): CodegenLayout {
  return layoutFor({ ...options, language: "rust" });
}

/** The module prefix that holds a generated artifact: `<root>::features::<dir>` under by-feature (vertical slice), else the flat `<root>::<top>::generated`. Rust imports are absolute paths, so the layout's relative `importSpecifier` doesn't apply — only the feature-dir casing is drawn from it. `root` is `crate` for in-crate `use`, or the package name for `tests/` integration crates. */
function generatedBase(
  entity: string,
  artifact: string,
  { byFeature, layout, root = "crate" }: RustUseOpts,
): string {
  if (byFeature)
    return `${root}::features::${layout.featureDir(entity, artifact)}`;
  return `${root}::${artifact === "route" ? "routes" : "services"}::generated`;
}

/** `use <root>::…::<service-module>::<symbol>;` for a generated service a router, sibling service, or test depends on. The module stem is the SDK-canonical service file base for the entity. */
export function serviceUseLine(
  entity: string,
  symbol: string,
  opts: RustUseOpts,
): string {
  const module = opts.layout.names.fileBase(entity, "service");
  return `use ${generatedBase(entity, "service", opts)}::${module}::${symbol};`;
}

/** `<root>::…::<routeModule>` — the entity's generated route module path (no trailing `::router`), for the app-wiring aggregator that calls each `router()` by absolute path to avoid glob collisions. */
export function routeModulePath(entity: string, opts: RustUseOpts): string {
  const routeModule = opts.layout.names.fileBase(entity, "route");
  return `${generatedBase(entity, "route", opts)}::${routeModule}`;
}

/** The app-wiring aggregator is cross-entity, so it can't key off the per-entity layout. Under by-feature it lives at `features/app_wiring.rs` (wired through `features/mod.rs`); flat leaves the stem bare and the routes step prefixes it into `routes/generated/`. A src-root file would never get a `pub mod` and orphan the composer. */
export function appWiringFilePath(byFeature: boolean): string {
  return byFeature ? "features/app_wiring.rs" : "app_wiring.rs";
}

/** The `crate::…::compose_router` path matching `appWiringFilePath` — lib.rs's `route_composer()` resolves the aggregator through it. */
export function appWiringComposePath(byFeature: boolean): string {
  return byFeature
    ? "crate::features::app_wiring::compose_router"
    : "crate::routes::generated::app_wiring::compose_router";
}
