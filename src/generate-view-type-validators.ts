import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { emitViewFields, inlinesParent, isAlias } from "./common/view-shape.ts";
import {
  DeterministicParser,
  VIEW_TYPES_YAML,
  type ShapedView,
  type ViewField,
  type ViewType,
  type IDeterministic,
} from "./specification-parser.ts";
import {
  checkArrayNullableTmpl,
  checkArrayTmpl,
  checkNullableTmpl,
  checkRequiredTmpl,
  typeTmpl,
} from "./resources/view-type-validators.ts";
import { Emit } from "./emit.ts";

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    const expandedByName = new Map(
      deterministic.expandedViewTypes.map((v) => [v.name, v]),
    );
    return deterministic.viewTypes.map((view) =>
      this.view(view, expandedByName.get(view.name)),
    );
  }

  private typePath(entity: string, kind: "datasource" | "view"): string {
    return kind === "datasource"
      ? this.imports.datasourceQual(entity)
      : this.imports.viewQual(entity);
  }

  private validatorFn(entity: string, kind: "datasource" | "view"): string {
    const fn =
      kind === "datasource"
        ? this.casing.convertFields(`validate_datasource_${entity}`)
        : this.casing.convertFields(`validate_${entity}`);
    return this.imports.validatorFn(kind, entity, fn);
  }

  private checkField(field: ViewField): string {
    const prop = this.casing.convertFields(field.name);
    const access = `obj.${prop}`;
    if (field.kind === "primitive") return "";
    const fn = this.validatorFn(field.base, field.kind);
    if (field.isArray) {
      const tmpl = field.isNullable ? checkArrayNullableTmpl : checkArrayTmpl;
      return fill(tmpl, { access, fn }).trimEnd();
    }
    if (field.isNullable) {
      return fill(checkNullableTmpl, { access, fn }).trimEnd();
    }
    return fill(checkRequiredTmpl, { fn, arg: `&${access}` }).trimEnd();
  }

  private shapedBody(
    view: ShapedView,
    expanded: ViewType | undefined,
  ): string[] {
    const checks: string[] = [];
    if (view.inherits !== null && !inlinesParent(view)) {
      const fn = this.validatorFn(view.inherits, "datasource");
      const arg = isAlias(view) ? "obj" : "&obj.base";
      checks.push(fill(checkRequiredTmpl, { fn, arg }).trimEnd());
    }
    for (const line of emitViewFields(view, expanded).map((f) =>
      this.checkField(f),
    )) {
      if (line !== "") checks.push(line);
    }
    return checks;
  }

  private view(
    view: ViewType,
    expanded: ViewType | undefined,
  ): GenerateEntry {
    const fnName = this.casing.convertFields(`validate_${view.name}`);
    const path = this.imports.viewValidator(view.name);
    if (view.kind === "union") {
      const cls = this.typePath(view.name, "view");
      return content(
        path,
        fill(typeTmpl, {
          schemaVersion: this.settings.schemaVersion,
          isUnion: true,
          isShaped: false,
          fnName,
          typePath: cls,
          arms: view.members.map((m) => ({
            arm: `${cls}::${this.casing.convertTypes(m)}(inner) => ${this.validatorFn(m, "view")}(inner),`,
          })),
          paramName: "obj",
          hasChecks: false,
          checks: [],
        }),
      );
    }
    const checks = this.shapedBody(view, expanded);
    return content(
      path,
      fill(typeTmpl, {
        schemaVersion: this.settings.schemaVersion,
        isUnion: false,
        isShaped: true,
        fnName,
        typePath: this.typePath(view.name, "view"),
        paramName: checks.length > 0 ? "obj" : "_obj",
        hasChecks: checks.length > 0,
        checks: checks.map((line) => ({ line })),
        arms: [],
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(VIEW_TYPES_YAML);
  return new Generator(ctx.settings).from(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  );
};
