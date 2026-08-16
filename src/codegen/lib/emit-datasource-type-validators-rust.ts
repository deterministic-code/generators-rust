import { createEmitter } from "./emit-datasource-validator-rust.ts";
import { makeDatasourceEmit } from "@deterministic-code/generator-sdk/codegen/lib/datasource-emit-config";

/** Self-describing emit for the rust datasource-type validators — wraps the shared `emit-datasource-validator-rust` render via `makeDatasourceEmit`. */
export const emit = makeDatasourceEmit(createEmitter, "rust");
