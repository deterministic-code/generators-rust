import { snakeCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { viewPaths } from "./common/paths.ts";
import {
  qual,
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

const testPath = (entity: string, naming: ViewTestOpts["naming"]): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const shapedCases = (view: ShapedView, opts: ViewTestOpts) => {
  const fields = shapedToks(view, opts, new Set([view.name]));
  const cls = opts.naming.className(view.name);
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
  const naming = viewPaths(settings);
  const opts: ViewTestOpts = {
    naming,
    tables: new Map(
      deterministic.expandedDatasourceTypes.map((t) => [t.name, t]),
    ),
    views: new Map(deterministic.viewTypes.map((v) => [v.name, v])),
    expandedViews: new Map(
      deterministic.expandedViewTypes.map((v) => [v.name, v]),
    ),
  };
  const schemaVersion = settings["codegen.schema_version"] ?? "1.0";
  return deterministic.viewTypes.map((view: ViewType) =>
    content(
      testPath(view.name, naming),
      fill(typeTestTmpl, {
        schemaVersion,
        typeUse: qual(view.name, "view", naming),
        fnName: `validate_${snakeCase(view.name)}`,
        cases:
          view.kind === "union"
            ? view.members.map((name) => ({
                ident: `accepts_${naming.fieldName(name)}_member`,
                fixture: `${naming.className(view.name)}::${naming.className(name)}(${viewExpr(name, opts, new Set([view.name]))})`,
                assertion: "is_ok()",
              }))
            : shapedCases(view, opts),
      }),
    ),
  );
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
