import { parse } from "yaml";
import { DATASOURCE_TYPES_YAML } from "./parse-datasource-types.ts";
import { isFiniteInt, isRecord, namedEntries } from "./yaml-entry.ts";

export const VIEW_TYPES_YAML = "view_types.yaml";

export type ViewFieldKind = "primitive" | "datasource" | "view";

export type ViewField = {
  name: string;
  type: string;
  kind: ViewFieldKind;
  base: string;
  isArray: boolean;
  isNullable: boolean;
  size?: number;
  minSize?: number;
};

export type ViewEnrichment = {
  fkColumn: string;
  prefix: string;
  targetTable: string;
  newField: string;
  targetIsReadonlyLookup: boolean;
  isNullable: boolean;
};

export type ShapedView = {
  kind: "shaped";
  name: string;
  inherits: string | null;
  fields: ViewField[];
  enrichments: ViewEnrichment[];
  omit: string[];
};

export type UnionView = {
  kind: "union";
  name: string;
  members: string[];
};

export type ViewType = ShapedView | UnionView;

const PRIMITIVES = new Set([
  "string", "character", "number", "integer", "smallinteger", "biginteger",
  "float", "decimal", "boolean", "datetime", "binary", "uuid", "reference",
]);
const DS_PREFIX = "datasource_types.";
const NON_DERIVABLE = [
  "_eager_body", "_eager_create_body", "_eager_patch_body",
  "_eager_row", "_eager_create_row",
] as const;

const rec = (v: unknown): Record<string, unknown> => (isRecord(v) ? v : {});
const str = (v: unknown): string | undefined =>
  typeof v === "string" ? v : undefined;
const int = (v: unknown): number | undefined =>
  typeof v === "number" && isFiniteInt(v) ? v : undefined;
const strings = (v: unknown): string[] =>
  Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];

export const parseFieldType = (
  raw: string,
): { kind: ViewFieldKind; base: string; isArray: boolean } => {
  const isArray = raw.endsWith("[]");
  const base = isArray ? raw.slice(0, -2) : raw;
  if (PRIMITIVES.has(base)) return { kind: "primitive", base, isArray };
  if (base.startsWith(DS_PREFIX)) {
    return { kind: "datasource", base: base.slice(DS_PREFIX.length), isArray };
  }
  return { kind: "view", base, isArray };
};

type DsField = {
  name: string;
  type: string | undefined;
  isNullable: boolean;
  isUnique: boolean;
  isPrimaryKey: boolean;
  references: string | undefined;
};
type DsType = {
  name: string;
  datasourceType: string | null;
  fields: DsField[];
};
type RawField = {
  name: string;
  type: string;
  isNullable: boolean;
  size: number | undefined;
  minSize: number | undefined;
};
type RawView = {
  name: string;
  inherits: string | undefined;
  oneOf: string[] | undefined;
  omit: string[];
  fields: RawField[];
  enrichments: ViewEnrichment[];
};
type DsDirective = {
  include: string | undefined;
  filter: string | undefined;
  autoEnrich: boolean;
};

const emptyShaped = (
  name: string,
  inherits: string,
  omit: string[],
): RawView => ({
  name,
  inherits,
  oneOf: undefined,
  omit,
  fields: [],
  enrichments: [],
});

const parseDsTypes = (yaml: string): DsType[] =>
  namedEntries(rec(parse(yaml)).types).map(([name, body]) => {
    const raw = rec(body);
    return {
      name,
      datasourceType: str(raw.datasource_type) ?? null,
      fields: namedEntries(raw.fields).map(([fname, fbody]) => {
        const f = rec(fbody);
        return {
          name: fname,
          type: str(f.type),
          isNullable: f.is_nullable === true,
          isUnique: f.is_unique === true,
          isPrimaryKey: f.primary_key === true,
          references: str(f.references),
        };
      }),
    };
  });

const parseRawViews = (yaml: string): RawView[] =>
  namedEntries(rec(parse(yaml)).types).map(([name, body]) => {
    const raw = rec(body);
    return {
      name,
      inherits: str(raw.inherits),
      oneOf: Array.isArray(raw.one_of) ? strings(raw.one_of) : undefined,
      omit: strings(raw.omit),
      fields: namedEntries(raw.fields).map(([fname, fbody]) => {
        const f = rec(fbody);
        return {
          name: fname,
          type: str(f.type) ?? "string",
          isNullable: f.is_nullable === true,
          size: int(f.size),
          minSize: int(f.min_size),
        };
      }),
      enrichments: [],
    };
  });

const datasourceDirective = (viewYaml: string): DsDirective | undefined => {
  for (const [key, body] of namedEntries(rec(parse(viewYaml)).includes)) {
    if (key !== "datasource_types") continue;
    const raw = rec(body);
    return {
      include: str(raw.include),
      filter: str(raw.filter),
      autoEnrich: raw.auto_enrich === true,
    };
  }
  return undefined;
};

const includeMatches = (include: string | undefined, name: string): boolean =>
  include === undefined ||
  include === "*" ||
  include.split(",").map((s) => s.trim()).filter(Boolean).includes(name);

const compileFilter = (
  filterExpr: string | undefined,
): ((t: { name: string; datasource_type: string | null }) => boolean) => {
  if (filterExpr === undefined) return () => true;
  try {
    const fn = new Function("type", `return (${filterExpr});`);
    return (t) => Boolean(fn(t));
  } catch (e) {
    throw new Error(
      `datasource_types.filter is not a valid expression: ${(e as Error).message}`,
    );
  }
};

const inheritedTable = (inherits: string | undefined): string | undefined =>
  inherits?.startsWith(DS_PREFIX) ? inherits.slice(DS_PREFIX.length) : undefined;

const parseFk = (field: DsField): string | undefined => {
  if (field.type !== "number" || field.references === undefined) return undefined;
  const [table, column] = field.references.split(".");
  return column === "id" ? table : undefined;
};

const targetIsEnrichable = (target: DsType | undefined): target is DsType => {
  if (target === undefined) return false;
  if (target.datasourceType === "readonly-lookup") return true;
  const name = target.fields.find((f) => f.name === "name");
  return (
    name !== undefined &&
    name.type === "string" &&
    name.isUnique &&
    !name.isNullable
  );
};

const enrichmentsFor = (
  tableName: string,
  byName: Map<string, DsType>,
): ViewEnrichment[] => {
  const inherited = byName.get(tableName);
  if (inherited === undefined) return [];
  return inherited.fields.flatMap((field) => {
    if (!field.name.endsWith("_id")) return [];
    const table = parseFk(field);
    if (table === undefined) return [];
    const target = byName.get(table);
    if (!targetIsEnrichable(target)) return [];
    const prefix = field.name.slice(0, -"_id".length);
    return [
      {
        fkColumn: field.name,
        prefix,
        targetTable: table,
        newField: `${prefix}_name`,
        targetIsReadonlyLookup: target.datasourceType === "readonly-lookup",
        isNullable: field.isNullable,
      },
    ];
  });
};

const applyAutoEnrich = (
  views: RawView[],
  byName: Map<string, DsType>,
): RawView[] =>
  views.map((view) => {
    const table = inheritedTable(view.inherits);
    if (table === undefined) return view;
    const enrichments = enrichmentsFor(table, byName);
    if (enrichments.length === 0) return view;
    return {
      ...view,
      enrichments,
      fields: [
        ...view.fields,
        ...enrichments.map((e) => ({
          name: e.newField,
          type: "string",
          isNullable: e.isNullable,
          size: undefined as number | undefined,
          minSize: undefined as number | undefined,
        })),
      ],
    };
  });

const isNonDerivable = (name: string): boolean =>
  NON_DERIVABLE.some((s) => name.endsWith(s)) ||
  name.startsWith("create_") ||
  name.startsWith("update_");

const auditOmits = (
  fields: DsField[],
): { updateBodyOmits: string[]; auditOmits: string[]; hasCustomPk: boolean } => {
  const declared = new Set(fields.map((f) => f.name));
  const missing = ["id", "uuid", "created", "updated"].filter(
    (n) => !declared.has(n),
  );
  const updateBodyOmits = [...missing];
  let hasCustomPk = false;
  for (const field of fields) {
    if (field.isPrimaryKey && field.name !== "id") {
      updateBodyOmits.push(field.name);
      hasCustomPk = true;
    }
  }
  return { updateBodyOmits, auditOmits: missing, hasCustomPk };
};

const updateVariantsFor = (
  view: RawView,
  byName: Map<string, DsType>,
  explicit: Set<string>,
): RawView[] => {
  const table = inheritedTable(view.inherits);
  if (table === undefined) return [];
  const ds = byName.get(table);
  if (ds === undefined || ds.datasourceType === "readonly-lookup") return [];
  if (isNonDerivable(view.name) || explicit.has(`update_${view.name}`)) {
    return [];
  }
  const omits = auditOmits(ds.fields);
  const inherits = `${DS_PREFIX}${table}`;
  const out = [emptyShaped(`update_${view.name}`, inherits, omits.updateBodyOmits)];
  if (omits.hasCustomPk && !explicit.has(`create_${view.name}`)) {
    out.push(emptyShaped(`create_${view.name}`, inherits, omits.auditOmits));
  }
  return out;
};

const passThroughs = (
  dsTypes: DsType[],
  directive: DsDirective,
  explicit: Set<string>,
): RawView[] => {
  const predicate = compileFilter(directive.filter);
  return dsTypes
    .filter(
      (ds) =>
        !explicit.has(ds.name) &&
        includeMatches(directive.include, ds.name) &&
        predicate({ name: ds.name, datasource_type: ds.datasourceType }),
    )
    .map((ds) => emptyShaped(ds.name, `${DS_PREFIX}${ds.name}`, []));
};

const normalize = (view: RawView): ViewType => {
  if (view.oneOf !== undefined) {
    return { kind: "union", name: view.name, members: view.oneOf };
  }
  return {
    kind: "shaped",
    name: view.name,
    inherits: inheritedTable(view.inherits) ?? null,
    fields: view.fields.map((f) => ({
      name: f.name,
      type: f.type,
      ...parseFieldType(f.type),
      isNullable: f.isNullable,
      ...(f.size !== undefined ? { size: f.size } : {}),
      ...(f.minSize !== undefined ? { minSize: f.minSize } : {}),
    })),
    enrichments: view.enrichments,
    omit: view.omit,
  };
};

export const parseViewTypes = (args: {
  viewYaml: string;
  datasourceYaml?: string;
}): ViewType[] => {
  const directive = datasourceDirective(args.viewYaml);
  if (directive !== undefined && !args.datasourceYaml) {
    throw new Error(
      "view_types.yaml declares an includes datasource_types directive but no datasource_types.yaml was provided.",
    );
  }
  const explicit = parseRawViews(args.viewYaml);
  const dsTypes = args.datasourceYaml ? parseDsTypes(args.datasourceYaml) : [];
  const byName = new Map(dsTypes.map((t) => [t.name, t]));
  const names = new Set(explicit.map((v) => v.name));
  let views = directive
    ? [...passThroughs(dsTypes, directive, names), ...explicit]
    : explicit;
  if (byName.size > 0) {
    const explicitNames = new Set(views.map((v) => v.name));
    views = views.flatMap((v) => [
      v,
      ...updateVariantsFor(v, byName, explicitNames),
    ]);
  }
  if (directive?.autoEnrich) views = applyAutoEnrich(views, byName);
  return views.map(normalize);
};

export const loadViewTypes = async (reader: {
  read: (name: string) => Promise<string>;
  exists: (name: string) => Promise<boolean>;
}): Promise<ViewType[]> => {
  const viewYaml = await reader.read(VIEW_TYPES_YAML);
  const datasourceYaml = (await reader.exists(DATASOURCE_TYPES_YAML))
    ? await reader.read(DATASOURCE_TYPES_YAML)
    : undefined;
  return parseViewTypes({ viewYaml, datasourceYaml });
};
