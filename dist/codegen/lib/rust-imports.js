import { toCase } from "@deterministic-code/generator-sdk/case";
import { layoutFor, namesFor, } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
/** The Rust lane's import authority, injected into an emitter as `ctx.imports`. Rust references sibling generated artifacts by fully-qualified module path; under the by-feature vertical slice each artifact lives in `crate::features::<slice>::<file-module>`, and under the flat business-concern layout in the shared `crate::types::generated::{datasource,views}[::validators]` re-export barrels. All cross-artifact paths derive from `ctx.layout`/`ctx.names`/`ctx.byFeature` so a single reference stays correct as casing or layout changes. */
export class RustImports {
    ctx;
    constructor(ctx) {
        this.ctx = ctx;
    }
    #featurePrefix(entity, artifact) {
        return `crate::features::${this.ctx.layout.featureDir(entity, artifact)}`;
    }
    /** The by-feature module (feature dir + file stem) an artifact compiles to — derived from the emitted file path so the reference can never drift from placement. */
    #featureModule(entity, artifact) {
        const path = this.ctx.layout.filePath(entity, artifact);
        const file = path.slice(path.lastIndexOf("/") + 1);
        const stem = file.slice(0, -this.ctx.names.ext.length);
        return `${this.#featurePrefix(entity, artifact)}::${stem}`;
    }
    /** A cross-artifact module path: the flat business-concern barrel, or the by-feature module when `ctx.byFeature`. */
    #barrel(entity, artifact, flat) {
        return this.ctx.byFeature ? this.#featureModule(entity, artifact) : flat;
    }
    /** Fully-qualified path to a generated datasource struct. */
    dsType(entity) {
        return `${this.dsTypeModule(entity)}::${this.ctx.names.className(entity)}`;
    }
    /** Fully-qualified path to a generated view struct. */
    viewType(entity) {
        return `${this.viewTypeModule(entity)}::${this.ctx.names.className(entity)}`;
    }
    /** Bare `<Class>Service` struct name — the Rust service role suffix on the SDK-derived class name (definition + local ctor sites). */
    serviceStruct(entity) {
        return `${this.ctx.names.className(entity)}Service`;
    }
    /** Bare `validate_datasource_<x>` fn name (definition site; Rust fns are always snake_case). */
    dsValidatorFnName(entity) {
        return `validate_datasource_${toCase(entity, "Snake")}`;
    }
    /** Bare `validate_<x>` fn name (definition site). */
    viewValidatorFnName(entity) {
        return `validate_${toCase(entity, "Snake")}`;
    }
    /** Fully-qualified path to a datasource type's `validate_datasource_<x>` fn (call site). */
    dsValidator(entity) {
        return `${this.dsValidatorModule(entity)}::${this.dsValidatorFnName(entity)}`;
    }
    /** Fully-qualified path to a view's `validate_<x>` fn (call site). */
    viewValidator(entity) {
        return `${this.viewValidatorModule(entity)}::${this.viewValidatorFnName(entity)}`;
    }
    /** Module path (no trailing symbol) holding a datasource struct — for `use <mod>::*` glob headers in test modules. */
    dsTypeModule(entity) {
        return this.#barrel(entity, "datasource-type", "crate::types::generated::datasource");
    }
    /** Module path holding a datasource type's validators — for `use <mod>::*` glob headers. */
    dsValidatorModule(entity) {
        return this.#barrel(entity, "datasource-validator", "crate::types::generated::datasource::validators");
    }
    /** Module path holding a view struct — for `use <mod>::*` glob headers. */
    viewTypeModule(entity) {
        return this.#barrel(entity, "view-type", "crate::types::generated::views");
    }
    /** Module path holding a view's validator — for `use <mod>::*` glob headers. */
    viewValidatorModule(entity) {
        return this.#barrel(entity, "view-validator", "crate::types::generated::views::validators");
    }
    /** Back-compat alias: a view field referencing a datasource struct. */
    qualified(base) {
        return this.dsType(base);
    }
    glob() {
        return "#[allow(unused_imports)]\nuse super::*;\n";
    }
}
/** Build a `RustImports` for a test emitter that runs outside an `EntityEmitter` ctx: rebuild layout/names from the resolved emit options so header `use` lines stay layout-aware. */
export function rustImportsForOptions(opts) {
    return new RustImports({
        layout: layoutFor(opts),
        names: namesFor(opts),
        byFeature: opts.organizeByFeature === true,
    });
}
