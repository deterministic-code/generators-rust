import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  authoredViewTypesOf,
  datasourceTypesOf,
  dictionaryEntryFields,
  dictionaryOfField,
  viewTypesOf,
} from "@deterministic-code/generators-common/spec-types";
import {
  emitViewFields,
  fieldRefKind,
  isAlias,
  isUnionEnum,
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
    if (field.isMap === true) {
      const dict = dictionaryOfField(field, this.typesByName);
      const entry = dict === undefined ? undefined : dictionaryEntryFields(dict);
      if (entry === undefined || entry.value.kind === "primitive") return "";
      const fn = this.validatorFn(
        entry.value.base,
        fieldRefKind(entry.value, this.typesByName),
      );
      return `    for item in ${access}.values() { if let Err(mut e) = ${fn}(item) { errors.append(&mut e); } }`;
    }
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
    const extras = emitViewFields(view, expanded, this.datasourceNames);
    if (isAlias(view) && extras.length === 0) {
      const fn = this.validatorFn(view.name, "datasource");
      checks.push(fill(checkRequiredTmpl, { fn, arg: "obj" }).trimEnd());
    } else if (isAlias(view)) {
      const fn = this.validatorFn(view.name, "datasource");
      checks.push(fill(checkRequiredTmpl, { fn, arg: "&obj.base" }).trimEnd());
    } else if (wrapsInheritedDatasource(view, this.datasourceNames)) {
      const fn = this.validatorFn(view.inherits!, "datasource");
      checks.push(fill(checkRequiredTmpl, { fn, arg: "&obj.base" }).trimEnd());
    }
    for (const line of extras.map((f) => this.checkField(f))) {
      if (line !== "") checks.push(line);
    }
    return checks;
  }

  private view(view: Type, expanded: Type | undefined): GenerateEntry {
    const fnName = this.casing.convertFields(`validate_${view.name}`);
    const path = this.imports.viewValidator(view.name);
    const isUnion = isUnionEnum(view);
    const enumName = this.casing.convertTypes(view.name);
    const arms = (view.union ?? []).map((name) => {
      const variant = this.casing.convertTypes(name);
      const fn = this.validatorFn(name, "view");
      return {
        arm: `${enumName}::${variant}(inner) => ${fn}(inner),`,
      };
    });
    const checks = isUnion ? [] : this.shapedBody(view, expanded);
    return content(
      path,
      fill(typeTmpl, {
        schemaVersion: this.settings.schemaVersion,
        isUnion,
        isShaped: !isUnion,
        fnName,
        typePath: this.typePath(view.name, "view"),
        paramName: checks.length > 0 ? "obj" : "_obj",
        hasChecks: checks.length > 0,
        checks: checks.map((line) => ({ line })),
        arms,
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
