import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { createImportGenerator } from "./import-generator.ts";
import {
  className,
  fieldName,
  shapedToks,
  viewExpr,
  type ViewTestOpts,
} from "./common/view-test-fixtures.ts";
import {
  DeterministicParser,
  VIEW_TYPES_YAML,
  type IDeterministic,
  type ViewType,
} from "./specification-parser.ts";
import { typeTestTmpl } from "./resources/view-types-tests.ts";

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const imports = createImportGenerator(".", settings);
  const opts: ViewTestOpts = {
    imports,
    tables: new Map(
      deterministic.expandedDatasourceTypes.map((t) => [t.name, t]),
    ),
    views: new Map(deterministic.viewTypes.map((v) => [v.name, v])),
    expandedViews: new Map(
      deterministic.expandedViewTypes.map((v) => [v.name, v]),
    ),
  };
  const schemaVersion = settings["codegen.schema_version"] ?? "1.0";
  return deterministic.viewTypes.map((view: ViewType) => {
    const fields =
      view.kind === "shaped"
        ? shapedToks(view, opts, new Set([view.name]))
        : [];
    const cls = className(view.name);
    const fixture =
      view.kind !== "shaped"
        ? ""
        : fields.length === 0
          ? `${cls} {}`
          : `${cls} {\n${fields.map((f) => `            ${f.ident}: ${f.sampleExpr},`).join("\n")}\n        }`;
    const src = imports.view(view.name);
    return content(
      imports.test(src, view.name),
      fill(typeTestTmpl, {
        schemaVersion,
        structName: cls,
        fileBase: view.name,
        isShaped: view.kind === "shaped",
        isUnion: view.kind === "union",
        fixture,
        fields,
        members:
          view.kind === "union"
            ? view.members.map((name) => ({
                ident: fieldName(name),
                memberExpr: `${cls}::${className(name)}(${viewExpr(name, opts, new Set([view.name]))})`,
              }))
            : [],
      }),
    );
  });
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(VIEW_TYPES_YAML);
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
    ctx.settings,
  );
};
