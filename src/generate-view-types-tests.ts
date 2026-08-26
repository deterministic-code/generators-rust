import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  authoredViewTypesOf,
  datasourceTypesOf,
  viewTypesOf,
} from "@deterministic-code/generators-common/spec-types";
import {
  shapedToks,
  type ViewTestOpts,
} from "./common/view-test-fixtures.ts";
import {
  DeterministicParser,
  TYPES_YAML,
  type IDeterministic,
  type Type,
} from "./specification-parser.ts";
import { typeTestTmpl } from "./resources/view-types-tests.ts";
import { Emit } from "./emit.ts";

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
    const fields = shapedToks(view, this, new Set([view.name]));
    const cls = this.casing.convertTypes(view.name);
    const fixture =
      fields.length === 0
        ? `${cls} {}`
        : `${cls} {\n${fields.map((f) => `            ${f.ident}: ${f.sampleExpr},`).join("\n")}\n        }`;
    const src = this.imports.view(view.name);
    return content(
      this.imports.test(src, view.name),
      fill(typeTestTmpl, {
        schemaVersion: this.settings.schemaVersion,
        structName: cls,
        fileBase: this.casing.fileBase(view.name),
        isShaped: true,
        isUnion: false,
        fixture,
        fields,
        members: [],
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
