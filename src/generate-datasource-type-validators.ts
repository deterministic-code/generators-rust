import { createGenerator } from "./generate-datasource-validator.ts";
import { makeDatasourceGenerate } from "@deterministic-code/generator-sdk/codegen/lib/datasource-generate-config";

/** Self-describing generate for the rust datasource-type validators — wraps the shared `generate-datasource-validator` render via `makeDatasourceGenerate`. */
export const generate = makeDatasourceGenerate(createGenerator, "rust");
