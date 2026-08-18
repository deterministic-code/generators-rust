const KINDS = ["datasource_type", "view_type", "service", "route"] as const;
const NAMESPACES = ["datasource_types"] as const;
const ALLOWED_INTERNAL = new Set([
  "__kind",
  "__ns",
  "__name",
  "true",
  "false",
  "null",
  "undefined",
]);

export type FilterCandidate = {
  name: string;
  kind: string;
  inheritsNamespace: string;
};

export type FilterPredicate = (cand: FilterCandidate) => boolean;

type PushPlaceholder = (raw: string) => string;
type CompiledExpr = (name: string, kind: string, ns: string) => boolean;

const escapeRegExp = (s: string): string =>
  s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Rewrite `type is/inherits/==` DSL into a JS boolean over `__name`/`__kind`/`__ns`. */
const rewriteDslToJs = (input: string, push: PushPlaceholder): string => {
  let s = input;
  for (const kind of KINDS) {
    s = s.replace(
      new RegExp(`\\btype\\s+is\\s+not\\s+${escapeRegExp(kind)}\\b`, "g"),
      () => `(__kind !== ${push(JSON.stringify(kind))})`,
    );
  }
  for (const kind of KINDS) {
    s = s.replace(
      new RegExp(`\\btype\\s+is\\s+${escapeRegExp(kind)}\\b`, "g"),
      () => `(__kind === ${push(JSON.stringify(kind))})`,
    );
  }
  for (const ns of NAMESPACES) {
    s = s.replace(
      new RegExp(`\\btype\\s+inherits\\s+not\\s+${escapeRegExp(ns)}\\b`, "g"),
      () => `(__ns !== ${push(JSON.stringify(ns))})`,
    );
  }
  for (const ns of NAMESPACES) {
    s = s.replace(
      new RegExp(`\\btype\\s+inherits\\s+${escapeRegExp(ns)}\\b`, "g"),
      () => `(__ns === ${push(JSON.stringify(ns))})`,
    );
  }
  return s.replace(/\btype\s*(==|!=)\s*__STR(\d+)__/g, (_, op, idx) => {
    const jsOp = op === "==" ? "===" : "!==";
    return `(__name ${jsOp} __STR${idx}__)`;
  });
};

const assertKnownIdents = (s: string, contextLabel: string): void => {
  for (const ident of s.match(/[A-Za-z_][A-Za-z0-9_]*/g) ?? []) {
    if (ident.startsWith("__STR")) continue;
    if (ALLOWED_INTERNAL.has(ident)) continue;
    throw new Error(
      `${contextLabel}: unknown identifier or syntax near "${ident}". Supported: \`type is [not] <kind>\`, \`type inherits [not] <namespace>\`, \`type == "name"\`, logical && / ||, parens. Kinds: ${KINDS.join(", ")}. Namespaces: ${NAMESPACES.join(", ")}.`,
    );
  }
};

const compileExpr = (s: string, contextLabel: string): CompiledExpr => {
  try {
    return new Function(
      "__name",
      "__kind",
      "__ns",
      `return (${s});`,
    ) as CompiledExpr;
  } catch (e) {
    throw new Error(
      `${contextLabel} is not a valid expression: ${(e as Error).message}`,
    );
  }
};

export const compileFilter = (
  filterExpr: string | null | undefined,
  contextLabel = "filter",
): FilterPredicate => {
  if (!filterExpr) return () => true;

  const placeholders: string[] = [];
  const push: PushPlaceholder = (raw) => {
    placeholders.push(raw);
    return `__STR${placeholders.length - 1}__`;
  };

  const withStrings = filterExpr.replace(/"[^"]*"/g, (m) => push(m));
  const rewritten = rewriteDslToJs(withStrings, push);
  assertKnownIdents(rewritten, contextLabel);
  const restored = rewritten.replace(
    /__STR(\d+)__/g,
    (_, idx) => placeholders[Number(idx)],
  );
  const fn = compileExpr(restored, contextLabel);
  return (cand) => Boolean(fn(cand.name, cand.kind, cand.inheritsNamespace));
};

export const compileServicesFilter = (
  filterExpr: string | null | undefined,
): FilterPredicate =>
  compileFilter(filterExpr, "view_type_services.filter");

export const compileRoutesFilter = (
  filterExpr: string | null | undefined,
): FilterPredicate =>
  compileFilter(filterExpr, "view_type_routes.filter");
