import type { NamesForOptions } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
import type { CodegenNames } from "@deterministic-code/generator-sdk/codegen-naming";
import type { CodegenLayout } from "@deterministic-code/generator-sdk/codegen-layout";
interface RustImportsCtx {
    layout: Pick<CodegenLayout, "featureDir" | "filePath">;
    names: Pick<CodegenNames, "className" | "ext">;
    byFeature: boolean;
}
/** The Rust lane's import authority, injected into an generator as `ctx.imports`. Rust references sibling generated artifacts by fully-qualified module path; under the by-feature vertical slice each artifact lives in `crate::features::<slice>::<file-module>`, and under the flat business-concern layout in the shared `crate::types::generated::{datasource,views}[::validators]` re-export barrels. All cross-artifact paths derive from `ctx.layout`/`ctx.names`/`ctx.byFeature` so a single reference stays correct as casing or layout changes. */
export declare class RustImports {
    #private;
    ctx: RustImportsCtx;
    constructor(ctx: RustImportsCtx);
    /** Fully-qualified path to a generated datasource struct. */
    dsType(entity: string): string;
    /** Fully-qualified path to a generated view struct. */
    viewType(entity: string): string;
    /** Bare `<Class>Service` struct name — the Rust service role suffix on the SDK-derived class name (definition + local ctor sites). */
    serviceStruct(entity: string): string;
    /** Bare `validate_datasource_<x>` fn name (definition site; Rust fns are always snake_case). */
    dsValidatorFnName(entity: string): string;
    /** Bare `validate_<x>` fn name (definition site). */
    viewValidatorFnName(entity: string): string;
    /** Fully-qualified path to a datasource type's `validate_datasource_<x>` fn (call site). */
    dsValidator(entity: string): string;
    /** Fully-qualified path to a view's `validate_<x>` fn (call site). */
    viewValidator(entity: string): string;
    /** Module path (no trailing symbol) holding a datasource struct — for `use <mod>::*` glob headers in test modules. */
    dsTypeModule(entity: string): string;
    /** Module path holding a datasource type's validators — for `use <mod>::*` glob headers. */
    dsValidatorModule(entity: string): string;
    /** Module path holding a view struct — for `use <mod>::*` glob headers. */
    viewTypeModule(entity: string): string;
    /** Module path holding a view's validator — for `use <mod>::*` glob headers. */
    viewValidatorModule(entity: string): string;
    /** Back-compat alias: a view field referencing a datasource struct. */
    qualified(base: string): string;
    glob(): string;
}
/** Build a `RustImports` for a test generator that runs outside an `EntityGenerator` ctx: rebuild layout/names from the resolved generate options so header `use` lines stay layout-aware. */
export declare function rustImportsForOptions(opts: NamesForOptions): RustImports;
export {};
