import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
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
import { Emit } from "./emit.ts";

class Generator extends Emit implements ViewTestOpts {
  readonly tables: ViewTestOpts["tables"];
  readonly views: ViewTestOpts["views"];
  readonly expandedViews: ViewTestOpts["expandedViews"];

  constructor(raw: Record<string, string>, deterministic: IDeterministic) {
    super(raw);
    this.tables = new Map(
      deterministic.expandedDatasourceTypes.map((t) => [t.name, t]),
    );
    this.views = new Map(deterministic.viewTypes.map((v) => [v.name, v]));
    this.expandedViews = new Map(
      deterministic.expandedViewTypes.map((v) => [v.name, v]),
    );
  }

  from(): GenerateEntry[] {
    return [...this.views.values()].map((view) => this.tests(view));
  }

  private tests(view: ViewType): GenerateEntry {
    const fields =
      view.kind === "shaped"
        ? shapedToks(view, this, new Set([view.name]))
        : [];
    const cls = this.casing.convertTypes(view.name);
    const fixture =
      view.kind !== "shaped"
        ? ""
        : fields.length === 0
          ? `${cls} {}`
          : `${cls} {\n${fields.map((f) => `            ${f.ident}: ${f.sampleExpr},`).join("\n")}\n        }`;
    const src = this.imports.view(view.name);
    return content(
      this.imports.test(src, view.name),
      fill(typeTestTmpl, {
        schemaVersion: this.settings.schemaVersion,
        structName: cls,
        fileBase: this.casing.fileBase(view.name),
        isShaped: view.kind === "shaped",
        isUnion: view.kind === "union",
        fixture,
        fields,
        members:
          view.kind === "union"
            ? view.members.map((name) => ({
                ident: this.casing.convertFields(name),
                acceptsMemberTest: this.casing.fnIdent(
                  `accepts_${name}_member`,
                ),
                memberExpr: `${cls}::${this.casing.convertTypes(name)}(${viewExpr(name, this, new Set([view.name]))})`,
              }))
            : [],
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(VIEW_TYPES_YAML);
  return new Generator(
    ctx.settings,
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  ).from();
};
