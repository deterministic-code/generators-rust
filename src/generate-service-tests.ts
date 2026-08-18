import type { GenerateContext } from "./common/generate-context.ts";
import type { GenerateEntry } from "./common/generate-entry.ts";

/** Retired: per-entity service unit tests used the removed inline `::new` API. */
export const generate = async (
  _ctx: GenerateContext,
): Promise<GenerateEntry[]> => [];
