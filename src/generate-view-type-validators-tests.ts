import { snakeCase } from "change-case";
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
  type ShapedView,
  type ViewType,
} from "./specification-parser.ts";
import { typeTestTmpl } from "./resources/view-type-validators-tests.ts";

const shapedCases = (view: ShapedView, opts: ViewTestOpts) => {
  const fields = shapedToks(view, opts, new Set([view.name]));
  const cls = className(view.name);
  const cases = [
    {
      ident: "parses_a_valid_payload",
      fixture: `${cls} { ${fields.map((f) => `${f.ident}: ${f.sampleExpr}`).join(", ")} }`,
      assertion: "is_ok()",
    },
  ];
  if (fields.some((f) => f.nullable)) {
    cases.push({
      ident: "accepts_none_for_nullable_fields",
      fixture: `${cls} { ${fields.map((f) => `${f.ident}: ${f.nullable ? "None" : f.sampleExpr}`).join(", ")} }`,
      assertion: "is_ok()",
    });
  }
  return cases;
};

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
    const src = imports.view(view.name);
    return content(
      imports.test(src, view.name),
      fill(typeTestTmpl, {
        schemaVersion,
        typeUse: opts.imports.viewQual(view.name),
        fnName: `validate_${snakeCase(view.name)}`,
        cases:
          view.kind === "union"
            ? view.members.map((name) => ({
                ident: `accepts_${fieldName(name)}_member`,
                fixture: `${className(view.name)}::${className(name)}(${viewExpr(name, opts, new Set([view.name]))})`,
                assertion: "is_ok()",
              }))
            : shapedCases(view, opts),
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
