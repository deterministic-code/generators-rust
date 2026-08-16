import { posix, basename, dirname, join, relative, sep } from "node:path";
import { PATCH } from "@deterministic-code/generator-sdk/codegen/lib/emit-result";
function toPosix(p) {
    return sep === "/" ? p : p.split(sep).join("/");
}
/** Walk a path string up to its nearest `src` segment (pure — no disk); null when there is none (e.g. backend_app's crate-root outDir, migrate, tests/). */
function findSrcRoot(leafDir) {
    let dir = leafDir;
    for (let i = 0; i < 12; i++) {
        if (basename(dir) === "src")
            return dir;
        const parent = dirname(dir);
        if (parent === dir)
            return null;
        dir = parent;
    }
    return null;
}
/** Struct name of a rust custom service (mirrors the stub emitter: `impl DynamicService` + a `pub struct`); null otherwise. */
function customServiceStruct(content) {
    if (!/\bimpl\s+DynamicService\s+for\b/.test(content))
        return null;
    const m = /\bpub\s+struct\s+([A-Za-z_][A-Za-z0-9_]*)\s*[;{(]/.exec(content);
    return m ? m[1] : null;
}
function toRustModIdent(fileBase) {
    return fileBase.replace(/-/g, "_");
}
function isTestModule(base) {
    return base.endsWith("_tests") || base.endsWith("-tests");
}
/** Router modules all export `pub fn router()` — glob re-exports would collide, so they stay path-addressed. */
function isRouterModule(base) {
    return (base === "router" || base.endsWith("_router") || base.endsWith("-router"));
}
const VALID_RUST_IDENT = /^[A-Za-z_][A-Za-z0-9_]*$/;
/** features/ and routes/ hold sibling modules that would collide under a glob re-export (order.rs + order_view.rs; the router() fns), so both stay pub-mod-only (path-addressed). */
function pathAddressed(srcRelDir) {
    const top = srcRelDir.split("/")[0];
    return top === "features" || top === "routes";
}
function modDecl(base, { reexport }) {
    const ident = toRustModIdent(base);
    const decl = VALID_RUST_IDENT.test(ident) && ident === base
        ? `pub mod ${ident};`
        : `#[path = "${base}.rs"] pub mod ${ident};`;
    if (isTestModule(base))
        return `#[cfg(test)]\nmod ${ident};`;
    if (reexport && !isRouterModule(base)) {
        return `${decl}\npub use ${ident}::*;`;
    }
    return decl;
}
/** One entry's own crate module path from its src-relative dir: services/custom -> crate::services::custom. */
function cratePathFor(srcRelDir, base) {
    const segs = srcRelDir === "" ? [] : srcRelDir.split("/");
    return ["crate", ...segs.map(toRustModIdent), toRustModIdent(base)].join("::");
}
function pieceEntry(target, content, section) {
    return section ? { target, content, section } : { target, content };
}
/** The pieces one source file contributes: its own mod.rs decl, `pub mod <dir>` up the parent chain (top → lib.rs MODULES), and, when it's a custom service, its lib.rs CUSTOM_SERVICES registration. */
function piecesForFile(srcRelPath, custom) {
    const dir = posix.dirname(srcRelPath);
    if (dir === ".")
        return [];
    const base = posix.basename(srcRelPath, ".rs");
    const out = [
        pieceEntry(posix.join(dir, "mod.rs"), modDecl(base, { reexport: !pathAddressed(dir) })),
    ];
    const segs = dir.split("/");
    for (let i = segs.length; i >= 1; i--) {
        const child = segs[i - 1];
        const decl = child === "tests"
            ? "#[cfg(test)]\nmod tests;"
            : `pub mod ${toRustModIdent(child)};`;
        const parent = segs.slice(0, i - 1).join("/");
        out.push(parent === ""
            ? pieceEntry("lib.rs", decl, "MODULES")
            : pieceEntry(posix.join(parent, "mod.rs"), decl));
    }
    if (custom) {
        const cratePath = cratePathFor(dir, base);
        out.push(pieceEntry("lib.rs", `let services = services.with(\n    "${custom.struct}",\n    std::sync::Arc::new(${cratePath}::${custom.struct}::new()),\n);`, "CUSTOM_SERVICES"));
    }
    return out;
}
/** The `mod.rs`/`lib.rs` patch pieces a rust step contributes for the source files it emits (targets are src-root-relative; the caller re-addresses them). Pure — no directory reads. */
export function rustModulePieces(srcRelPaths, customServices = new Map()) {
    return srcRelPaths.flatMap((p) => piecesForFile(p, customServices.get(p)));
}
/** mod.rs is a crate module iff it sits under a `src/` root, outside the bundled library and the bin/ tree; src-root files (lib.rs/main.rs) and mod.rs itself are never wired. */
export function isWireableRustFile(srcRelPath) {
    const b = posix.basename(srcRelPath);
    if (b === "mod.rs" || b === "lib.rs" || b === "main.rs")
        return false;
    if (!b.endsWith(".rs"))
        return false;
    const dir = posix.dirname(srcRelPath);
    if (dir === ".")
        return false;
    if (dir.split("/")[0] === "bin")
        return false;
    return true;
}
/**
 * The `{kind:PATCH}` module-wiring entries a rust step contributes for the files
 * it just emitted (`files: {path, content}` with `path` relative to `outDir`).
 * Derives everything from that emitted list — never reads the directory. Returns
 * `[]` when the step's output isn't under a crate `src/` (backend_app's crate
 * root, migrate, perf bin, tests/).
 */
export function rustModuleEntriesFrom({ outDir, files, }) {
    const srcRoot = findSrcRoot(outDir);
    if (!srcRoot)
        return [];
    const srcRelByFile = new Map();
    const customServices = new Map();
    for (const f of files) {
        const abs = join(outDir, f.path);
        if (toPosix(abs).includes("/_deterministic/"))
            continue;
        const srcRel = toPosix(relative(srcRoot, abs));
        if (srcRel.startsWith("..") || !isWireableRustFile(srcRel))
            continue;
        srcRelByFile.set(f, srcRel);
        // Only files under a `custom/` dir register as custom services (generic service files also `impl DynamicService`, but are wired by services.yaml, not `services.with(...)`).
        if (/(^|\/)custom$/.test(posix.dirname(srcRel))) {
            const struct = customServiceStruct(f.content ?? "");
            if (struct)
                customServices.set(srcRel, { struct });
        }
    }
    const pieces = rustModulePieces([...srcRelByFile.values()], customServices);
    return pieces.map((p) => ({
        kind: PATCH,
        filename: toPosix(relative(outDir, join(srcRoot, p.target))),
        content: p.content,
        ...(p.section ? { section: p.section } : {}),
    }));
}
