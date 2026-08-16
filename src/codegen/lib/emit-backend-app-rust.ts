/** Catalog entry for the rust `backend_app` step. The render module — driver + every Cargo/Dockerfile/dialect helper — lives at `scripts/lib/emit-backend-app-rust.ts` (its ~15 relative imports and cross-module consumers keep it there); this module is the convention filename the `emit.mjs` runner dynamic-imports, and it exports the self-describing surface over that driver. `rustModuleWiring = false`: backend_app emits `src/lib.rs` with an empty MODULES marker block that later per-module steps patch, so the generic mod.rs pass must not run. */

import { emitBackendApp } from "../../lib/emit-backend-app-rust.ts";
import { makeBackendAppEmit } from "@deterministic-code/generator-sdk/codegen/lib/backend-app-emit-helpers";
import { COMBINED_FLAG } from "@deterministic-code/generator-sdk/codegen/lib/backend-lane";

export const emit = makeBackendAppEmit(emitBackendApp, "rust");
export const entriesNative = true;
export const pinProjectRoot = true;
export const flags = [COMBINED_FLAG];
export const rustModuleWiring = false;
