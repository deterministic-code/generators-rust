import { isCollectionField } from "@deterministic-code/generators-common/spec-types";
import { typeHasTag, type Type, type TypeField } from "../specification-parser.ts";

export const isAlias = (view: Type): boolean =>
  typeHasTag(view, "datasource_type");

export const isUnionEnum = (view: Type): boolean =>
  view.kind === "union" && (view.union?.length ?? 0) > 0;

export const emitViewFields = (
  view: Type,
  expanded: Type | undefined,
): TypeField[] => {
  if (isAlias(view)) {
    return (expanded?.fields ?? view.fields).filter(isCollectionField);
  }
  return expanded?.fields ?? view.fields;
};

export const fieldRefKind = (
  field: TypeField,
  typesByName: Map<string, Type>,
): "primitive" | "datasource" | "view" => {
  if (field.kind === "primitive") return "primitive";
  const referenced = typesByName.get(field.base);
  if (referenced === undefined) return "view";
  if (
    typeHasTag(referenced, "view_type") &&
    !typeHasTag(referenced, "datasource_type")
  ) {
    return "view";
  }
  return "datasource";
};
