import type { GenerateContext } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import { e2eTmpl } from "./resources/perf-e2e.ts";

export const generate = async (
  _ctx: GenerateContext,
): Promise<GenerateEntry[]> => [content("app_perf_client.rs", e2eTmpl)];
