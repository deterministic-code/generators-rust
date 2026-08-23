import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  authoredViewTypesOf,
  datasourceTypesOf,
  unionMembers,
  viewTypesOf,
} from "@deterministic-code/generators-common/spec-types";
import {
  shapedToks,
  viewExpr,
  type ViewTestOpts,
} from "./common/view-test-fixtures.ts";
import {
  DeterministicParser,
  TYPES_YAML,
  type IDeterministic,
  type Type,
} from "./specification-parser.ts";
import { typeTestTmpl } from "./resources/view-type-validators-tests.ts";
import { Emit } from "./emit.ts";

const shapedCases = (view: Type, opts: ViewTestOpts) => {
  const fields = shapedToks(view, opts, new Set([view.name]));
  const cls = opts.casing.convertTypes(view.name);
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

class Generator extends Emit implements ViewTestOpts {
  readonly tables: ViewTestOpts["tables"];
  readonly views: ViewTestOpts["views"];
  readonly expandedViews: ViewTestOpts["expandedViews"];
  readonly typesByName: ViewTestOpts["typesByName"];
  readonly datasourceNames: ViewTestOpts["datasourceNames"];

  constructor(raw: Record<string, string>, deterministic: IDeterministic) {
    super(raw);
    this.tables = new Map(
      datasourceTypesOf(deterministic).map((t) => [t.name, t]),
    );
    this.views = new Map(
      authoredViewTypesOf(deterministic).map((v) => [v.name, v]),
    );
    this.expandedViews = new Map(
      viewTypesOf(deterministic).map((v) => [v.name, v]),
    );
    this.typesByName = new Map(
      deterministic.expandedTypes.map((t) => [t.name, t]),
    );
    this.datasourceNames = new Set(this.tables.keys());
  }

  from(): GenerateEntry[] {
    return [...this.views.values()].map((view) => this.tests(view));
  }

  private tests(view: Type): GenerateEntry {
    const src = this.imports.view(view.name);
    const members = unionMembers(view);
    return content(
      this.imports.test(src, view.name),
      fill(typeTestTmpl, {
        schemaVersion: this.settings.schemaVersion,
        typeUse: this.imports.viewQual(view.name),
        fnName: this.casing.convertFields(`validate_${view.name}`),
        cases:
          members !== undefined
            ? members.map((name) => ({
                ident: this.casing.convertFields(`accepts_${name}_member`),
                fixture: `${this.casing.convertTypes(view.name)}::${this.casing.convertTypes(name)}(${viewExpr(name, this, new Set([view.name]))})`,
                assertion: "is_ok()",
              }))
            : shapedCases(view, this),
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(TYPES_YAML);
  return new Generator(
    ctx.settings,
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  ).from();
};
