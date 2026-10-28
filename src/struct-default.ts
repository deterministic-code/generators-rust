import {
  authoredViewTypesOf,
  datasourceTypesOf,
  fieldTypeOf,
  persistedColumnFields,
  viewTypesOf,
} from "@deterministic-code/generators-common/spec-types";
import type {
  IDeterministic,
  Type,
  TypeField,
} from "./specification-parser.ts";
import {
  emitViewFields,
  isAlias,
  isUnionEnum,
} from "./common/view-shape.ts";
import { rustSpecDefaultExpr } from "./deterministic-type-converter.ts";

const hasNonZeroDefault = (
  fields: readonly TypeField[],
  typesByName: ReadonlyMap<string, Type>,
): boolean =>
  fields.some(
    (field) =>
      field.kind === "primitive" &&
      rustSpecDefaultExpr(fieldTypeOf(field, typesByName), field) !== undefined,
  );

const directNested = (fields: readonly TypeField[]): string[] =>
  fields
    .filter(
      (field) =>
        field.kind === "type" &&
        !field.isArray &&
        field.isMap !== true &&
        !field.isNullable,
    )
    .map((field) => field.base);

export const typesNeedingDefault = (deterministic: IDeterministic): Set<string> => {
  const typesByName = new Map(
    deterministic.expandedTypes.map((type) => [type.name, type]),
  );
  const expandedViews = new Map(
    viewTypesOf(deterministic).map((view) => [view.name, view]),
  );
  const nestedByType = new Map<string, string[]>();
  const seeds: string[] = [];

  const consider = (name: string, fields: readonly TypeField[]): void => {
    nestedByType.set(name, directNested(fields));
    if (hasNonZeroDefault(fields, typesByName)) seeds.push(name);
  };

  for (const table of datasourceTypesOf(deterministic)) {
    consider(table.name, persistedColumnFields(table, typesByName));
  }

  for (const view of authoredViewTypesOf(deterministic)) {
    if (isUnionEnum(view)) continue;
    const expanded = expandedViews.get(view.name);
    const fields = emitViewFields(view, expanded);
    if (isAlias(view) && fields.length === 0) continue;
    consider(view.name, fields);
  }

  const needing = new Set(seeds);
  let grew = true;
  while (grew) {
    grew = false;
    for (const [name, nested] of nestedByType) {
      if (!needing.has(name)) continue;
      for (const nestedName of nested) {
        if (!nestedByType.has(nestedName) || needing.has(nestedName)) continue;
        needing.add(nestedName);
        grew = true;
      }
    }
  }
  return needing;
};
