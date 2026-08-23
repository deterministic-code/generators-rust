import { typeHasTag, type Type, type TypeField } from "../specification-parser.ts";

export const isAlias = (view: Type): boolean =>
  typeHasTag(view, "datasource_type");

export const wrapsInheritedDatasource = (
  view: Type,
  datasourceNames: Set<string>,
): boolean =>
  !isAlias(view) &&
  view.kind === "inherit" &&
  view.inherits !== undefined &&
  datasourceNames.has(view.inherits) &&
  (view.removeFields?.length ?? 0) === 0;

export const emitViewFields = (
  view: Type,
  expanded: Type | undefined,
  datasourceNames: Set<string>,
): TypeField[] => {
  if (isAlias(view)) return [];
  if (wrapsInheritedDatasource(view, datasourceNames)) return view.fields;
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
