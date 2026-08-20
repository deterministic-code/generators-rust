import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate as generateJson } from "@deterministic-code/generators-openapi/generate-openapi";
import { conformanceTmpl, routerTmpl } from "./resources/openapi.ts";

const escapeRustStringLiteral = (s: string): string =>
  s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n");

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const crateName = ctx.settings["languages.rust.crate_name"] ?? "consumer";
  const [jsonEntry] = await generateJson(ctx);
  if (jsonEntry === undefined || jsonEntry.kind !== "content") {
    throw new Error("openapi json lane did not emit openapi.json");
  }
  const spec = JSON.parse(jsonEntry.contents) as {
    paths?: Record<string, unknown>;
  };
  const expectedPaths = Object.keys(spec.paths ?? {})
    .sort()
    .map((p) => `        ${JSON.stringify(p)},`)
    .join("\n");
  return [
    content(
      "openapi.rs",
      fill(routerTmpl, {
        specJson: escapeRustStringLiteral(jsonEntry.contents.trim()),
      }),
    ),
    content(
      "openapi_conformance.rs",
      fill(conformanceTmpl, {
        crateModule: crateName.replace(/-/g, "_"),
        expectedPaths,
      }),
    ),
  ];
};
