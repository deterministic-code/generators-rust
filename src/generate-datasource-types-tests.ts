import { createGenerator } from "./generate-datasource-tests.ts";
import { makeDatasourceGenerate } from "@deterministic-code/generator-sdk/codegen/lib/datasource-generate-config";

/** Self-describing generate for the rust datasource-type tests — wraps the shared `generate-datasource-tests` render via `makeDatasourceGenerate`. */
export const generate = makeDatasourceGenerate(createGenerator, "rust");
