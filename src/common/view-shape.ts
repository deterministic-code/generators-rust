import type {
  ShapedView,
  ViewField,
  ViewType,
} from "../specification-parser.ts";

export const inlinesParent = (view: ShapedView): boolean =>
  view.inherits !== null &&
  (view.enrichments.length > 0 || view.omit.length > 0);

export const isAlias = (view: ShapedView): boolean =>
  view.inherits !== null &&
  view.fields.length === 0 &&
  view.enrichments.length === 0 &&
  view.omit.length === 0;

/** Field list to emit: expanded when inlining, otherwise authored extras. */
export const emitViewFields = (
  view: ShapedView,
  expanded: ViewType | undefined,
): ViewField[] => {
  if (isAlias(view)) return [];
  if (inlinesParent(view) && expanded?.kind === "shaped") return expanded.fields;
  return view.fields;
};
