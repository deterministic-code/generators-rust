import { createEmitter } from "./emit-datasource-tests-rust.ts";
import { makeDatasourceEmit } from "@deterministic-code/generator-sdk/codegen/lib/datasource-emit-config";

/** Self-describing emit for the rust datasource-type tests — wraps the shared `emit-datasource-tests-rust` render via `makeDatasourceEmit`. */
export const emit = makeDatasourceEmit(createEmitter, "rust");
