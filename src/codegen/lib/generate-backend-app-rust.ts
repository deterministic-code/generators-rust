/** Catalog entry for the rust `backend_app` step. The render module — driver + every Cargo/Dockerfile/dialect helper — lives at `scripts/lib/generate-backend-app-rust.ts` (its ~15 relative imports and cross-module consumers keep it there); this module is the convention filename the `generate.mjs` runner dynamic-imports, and it exports the self-describing surface over that driver. `rustModuleWiring = false`: backend_app generates `src/lib.rs` with an empty MODULES marker block that later per-module steps patch, so the generic mod.rs pass must not run. */

import { generateBackendApp } from "../../lib/generate-backend-app-rust.ts";
import { makeBackendAppGenerate } from "@deterministic-code/generator-sdk/codegen/lib/backend-app-generate-helpers";
import { COMBINED_FLAG } from "@deterministic-code/generator-sdk/codegen/lib/backend-lane";

export const generate = makeBackendAppGenerate(generateBackendApp, "rust");
export const entriesNative = true;
export const pinProjectRoot = true;
export const flags = [COMBINED_FLAG];
export const rustModuleWiring = false;
