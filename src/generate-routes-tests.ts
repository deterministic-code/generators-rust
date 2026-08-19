import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";

/** Retired: per-entity router tests used the removed `::new` API. */
export const generate = async (
  _ctx: GenerateContext,
): Promise<GenerateEntry[]> => [];
