import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, patch, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  DATASOURCE_TYPES_YAML,
} from "./specification-parser.ts";
import { serverTmpl } from "./resources/perf-server.ts";

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  if (!(await ctx.reader.exists(DATASOURCE_TYPES_YAML))) {
    throw new Error("generate-perf-server: datasource_types.yaml is required");
  }
  await ctx.reader.read(DATASOURCE_TYPES_YAML);
  return [
    content("src/bin/perf_server.rs", serverTmpl),
    patch(
      "Cargo.toml",
      '[[bin]]\nname = "perf_server"\npath = "src/bin/perf_server.rs"',
      "PERF_BIN",
    ),
  ];
};
