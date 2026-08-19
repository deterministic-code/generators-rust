import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { e2eTmpl } from "./resources/perf-e2e.ts";

export const generate = async (
  _ctx: GenerateContext,
): Promise<GenerateEntry[]> => [content("app_perf_client.rs", e2eTmpl)];
