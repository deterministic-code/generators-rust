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
  emitViewFields,
  fieldRefKind,
  isAlias,
  wrapsInheritedDatasource,
} from "./common/view-shape.ts";
import {
  DeterministicParser,
  TYPES_YAML,
  type IDeterministic,
  type Type,
  type TypeField,
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
  private readonly typesByName: Map<string, Type>;
  private readonly datasourceNames: Set<string>;
  private readonly expandedByName: Map<string, Type>;

  constructor(raw: Record<string, string>, deterministic: IDeterministic) {
    super(raw);
    this.typesByName = new Map(
      deterministic.expandedTypes.map((t) => [t.name, t]),
    );
    this.datasourceNames = new Set(
      datasourceTypesOf(deterministic).map((t) => t.name),
    );
    this.expandedByName = new Map(
      viewTypesOf(deterministic).map((v) => [v.name, v]),
    );
  }

  from(deterministic: IDeterministic): GenerateEntry[] {
    return authoredViewTypesOf(deterministic).map((view) =>
      this.view(view, this.expandedByName.get(view.name)),
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

  private checkField(field: TypeField): string {
    const prop = this.casing.convertFields(field.name);
    const access = `obj.${prop}`;
    const refKind = fieldRefKind(field, this.typesByName);
    if (refKind === "primitive") return "";
    const fn = this.validatorFn(field.base, refKind);
    if (field.isArray) {
      const tmpl = field.isNullable ? checkArrayNullableTmpl : checkArrayTmpl;
      return fill(tmpl, { access, fn }).trimEnd();
    }
    if (field.isNullable) {
      return fill(checkNullableTmpl, { access, fn }).trimEnd();
    }
    return fill(checkRequiredTmpl, { fn, arg: `&${access}` }).trimEnd();
  }

  private shapedBody(view: Type, expanded: Type | undefined): string[] {
    const checks: string[] = [];
    if (isAlias(view)) {
      const fn = this.validatorFn(view.name, "datasource");
      checks.push(fill(checkRequiredTmpl, { fn, arg: "obj" }).trimEnd());
    } else if (wrapsInheritedDatasource(view, this.datasourceNames)) {
      const fn = this.validatorFn(view.inherits!, "datasource");
      checks.push(fill(checkRequiredTmpl, { fn, arg: "&obj.base" }).trimEnd());
    }
    for (const line of emitViewFields(
      view,
      expanded,
      this.datasourceNames,
    ).map((f) => this.checkField(f))) {
      if (line !== "") checks.push(line);
    }
    return checks;
  }

  private view(view: Type, expanded: Type | undefined): GenerateEntry {
    const fnName = this.casing.convertFields(`validate_${view.name}`);
    const path = this.imports.viewValidator(view.name);
    const members = unionMembers(view);
    if (members !== undefined) {
      const cls = this.typePath(view.name, "view");
      return content(
        path,
        fill(typeTmpl, {
          schemaVersion: this.settings.schemaVersion,
          isUnion: true,
          isShaped: false,
          fnName,
          typePath: cls,
          arms: members.map((m) => ({
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
  await ctx.reader.read(TYPES_YAML);
  const deterministic = await DeterministicParser(ctx.reader).parse(
    ctx.settings,
  );
  return new Generator(ctx.settings, deterministic).from(deterministic);
};
