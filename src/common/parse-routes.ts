import { snakeCase } from "change-case";
import pluralize from "pluralize";
import { parse } from "yaml";
import { compileRoutesFilter } from "./compile-filter.ts";
import {
  DATASOURCE_TYPES_YAML,
  parseDatasourceTypes,
  type DatasourceType,
} from "./parse-datasource-types.ts";
import {
  loadViewTypes,
  VIEW_TYPES_YAML,
  type ViewType,
} from "./parse-view-types.ts";
import { isRecord, namedEntries } from "./yaml-entry.ts";

export const ROUTES_YAML = "routes.yaml";
export const SERVICES_YAML = "services.yaml";

export type RouteByField = {
  byField: string;
  methods?: string[];
  byFieldUnique: boolean;
};

export type RouteCandidate = {
  name: string;
  kind: "datasource_type" | "view_type";
  inheritsNamespace: string;
  datasourceType: string;
  target: string | null;
  optimisticConcurrency?: boolean;
  byFields: RouteByField[];
};

export type CustomRouteEntry = {
  /** Original single-key route map entry, e.g. { getHealth: { ... } } */
  entry: Record<string, unknown>;
  name: string;
};

export type DirectFkDescriptor = {
  kind: "direct-fk";
  parent: string;
  parentParam: string;
  parentBasePath: string;
  child: { name: string };
  fkColumn: string;
  segment: string;
  segmentTail: string;
};

export type M2mDescriptor = {
  kind: "m2m";
  parent: string;
  parentParam: string;
  parentBasePath: string;
  junction: string;
  target: string;
  targetParam: string;
  parentFkField: string;
  childFkField: string;
  segment: string;
  segmentTail: string;
};

export type NestedRouteDescriptor = DirectFkDescriptor | M2mDescriptor;

export type ParsedRoutes = {
  candidates: RouteCandidate[];
  customs: CustomRouteEntry[];
  nested: NestedRouteDescriptor[];
  childrenOnly: Set<string>;
  datasources: DatasourceType[];
};

const HEALTH_ROUTE_PATH = "/api/health";
const HEALTH_SERVICE_NAME = "HealthCheckService";
const SHORTHAND_VERB_RE = /^(get|put|delete)_/i;
const VERB_TO_METHODS: Record<string, string[]> = {
  get: ["GET"],
  put: ["PUT"],
  delete: ["DELETE"],
};

type RoutesDoc = Record<string, unknown>;
type CombinedChildDef = { via?: string; target?: string; route?: string };
type CombinedRouteDef = {
  route?: string;
  combined_types?: Array<string | Record<string, CombinedChildDef>>;
};
type ByFieldParsed = {
  entity: string;
  byField: string;
  methods: string[] | null;
};
type NormalizedChild = {
  name: string;
  via: string | null;
  target: string | null;
  route: string | null;
};

const rec = (value: unknown): Record<string, unknown> =>
  isRecord(value) ? value : {};

const str = (value: unknown): string | undefined =>
  typeof value === "string" ? value : undefined;

const snakeToCamel = (name: string): string =>
  name
    .split(/[_-]/)
    .map((part, i) =>
      i === 0 ? part : part.charAt(0).toUpperCase() + part.slice(1),
    )
    .join("");

const kebabToSnake = (value: string): string => value.replace(/-/g, "_");

const kebabPlural = (name: string): string => {
  const kebab = name.replace(/_/g, "-");
  const parts = kebab.split("-");
  parts[parts.length - 1] = pluralize.plural(parts[parts.length - 1]!);
  return parts.join("-");
};

const parentParamName = (parentName: string): string =>
  `${snakeToCamel(parentName)}Id`;

const rewriteParentPath = (rawPath: string, parentName: string): string =>
  rawPath.replace(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g, (_match, name: string) =>
    name === "id" ? `:${parentParamName(parentName)}` : `:${name}`,
  );

const defaultParentBasePath = (parentName: string): string =>
  rewriteParentPath(`/api/${kebabPlural(parentName)}/{id}`, parentName);

const segmentTailOf = (segment: string): string =>
  segment.split("/").filter(Boolean).pop() ?? "";

const defaultChildSegment = (name: string): string => `/${kebabPlural(name)}`;

const findHealthRouteIndex = (routes: unknown[]): number => {
  for (let i = 0; i < routes.length; i++) {
    const entry = routes[i];
    if (!isRecord(entry)) continue;
    for (const def of Object.values(entry)) {
      if (isRecord(def) && def.path === HEALTH_ROUTE_PATH) return i;
    }
  }
  return -1;
};

const ensureHealthRouteFirst = (routesDoc: unknown): RoutesDoc => {
  const seed = {
    getHealth: {
      method: "GET",
      path: HEALTH_ROUTE_PATH,
      service: HEALTH_SERVICE_NAME,
      serviceMethod: "check",
    },
  };
  if (!isRecord(routesDoc)) return { routes: [seed] };
  const routes = Array.isArray(routesDoc.routes) ? routesDoc.routes : [];
  const idx = findHealthRouteIndex(routes);
  if (idx === 0) return { ...routesDoc, routes: [...routes] };
  if (idx > 0) {
    return {
      ...routesDoc,
      routes: [
        routes[idx]!,
        ...routes.slice(0, idx),
        ...routes.slice(idx + 1),
      ],
    };
  }
  return { ...routesDoc, routes: [seed, ...routes] };
};

const routeViewTypeDirective = (
  doc: RoutesDoc,
): { filter?: string } | null => {
  const includes = Array.isArray(doc.includes) ? doc.includes : [];
  for (const entry of includes) {
    const raw = rec(entry);
    if (raw.view_type_routes !== undefined) {
      const block = rec(raw.view_type_routes);
      return { filter: str(block.filter) };
    }
  }
  return null;
};

const hasNoRouteSurface = (candidate: { target?: string | null }): boolean =>
  candidate.target === "None";

export const entityUsesOptimisticConcurrency = (
  table: { datasourceType?: string | null; optimisticConcurrency?: boolean },
  globalFlag: boolean,
): boolean => {
  if (table.datasourceType === "many-to-many") return false;
  if (table.datasourceType === "readonly-lookup") return false;
  if (table.optimisticConcurrency !== undefined) {
    return table.optimisticConcurrency;
  }
  return globalFlag === true;
};

const columnIsUnique = (
  ds: DatasourceType,
  columnName: string,
): boolean => {
  if (columnName === "id") return true;
  const field = ds.fields.find((f) => f.name === columnName);
  if (field?.isPrimaryKey === true || field?.isUnique === true) return true;
  return ds.uniqueIndexFields.includes(columnName);
};

const singularizeLastToken = (snakePlural: string): string => {
  const parts = snakePlural.split("_");
  if (parts.length === 0) return snakePlural;
  parts[parts.length - 1] = pluralize.singular(parts[parts.length - 1]!);
  return parts.join("_");
};

const entityHasField = (ds: DatasourceType, fieldName: string): boolean => {
  if (fieldName === "id") return true;
  return ds.fields.some((f) => f.name === fieldName);
};

const parseVerb = (
  token: string,
): { methods: string[] | null; body: string } => {
  const verbMatch = SHORTHAND_VERB_RE.exec(token);
  if (!verbMatch) return { methods: null, body: token };
  return {
    methods: VERB_TO_METHODS[verbMatch[1]!.toLowerCase()] ?? null,
    body: token.slice(verbMatch[0].length),
  };
};

const splitEntityField = (
  token: string,
  body: string,
): { entity: string; byField: string } => {
  const splitIdx = body.lastIndexOf("_by_");
  if (splitIdx < 0) {
    throw new Error(
      `parseByFieldEntry: route key \`${token}\` is missing \`_by_\` separator`,
    );
  }
  const pluralSnake = body.slice(0, splitIdx);
  const camelField = body.slice(splitIdx + "_by_".length);
  if (!pluralSnake || !camelField) {
    throw new Error(
      `parseByFieldEntry: route key \`${token}\` has empty entity or field around \`_by_\``,
    );
  }
  return {
    entity: singularizeLastToken(pluralSnake),
    byField: snakeCase(camelField),
  };
};

const parseShorthandByField = (
  token: string,
  dsByName: Map<string, DatasourceType>,
): ByFieldParsed => {
  if (typeof token !== "string" || token.length === 0) {
    throw new Error("parseByFieldEntry: expected non-empty string token");
  }
  const { methods, body } = parseVerb(token);
  const { entity, byField } = splitEntityField(token, body);
  const ds = dsByName.get(entity);
  if (ds === undefined) {
    throw new Error(
      `parseByFieldEntry: unknown entity \`${entity}\` in route \`${token}\``,
    );
  }
  if (!entityHasField(ds, byField)) {
    throw new Error(
      `parseByFieldEntry: field \`${byField}\` not found on entity \`${entity}\` in route \`${token}\``,
    );
  }
  return { entity, byField, methods };
};

const parseVerboseByField = (
  key: string,
  def: Record<string, unknown>,
): ByFieldParsed => {
  if (typeof def.entity !== "string" || typeof def.byField !== "string") {
    throw new Error(
      `parseByFieldEntry: route \`${key}\` has non-string entity/byField`,
    );
  }
  return {
    entity: def.entity,
    byField: def.byField,
    methods: Array.isArray(def.methods) ? def.methods : null,
  };
};

const parseByFieldEntry = (
  entry: unknown,
  dsByName: Map<string, DatasourceType>,
): ByFieldParsed | null => {
  if (entry == null) return null;
  if (typeof entry === "string") {
    return parseShorthandByField(entry, dsByName);
  }
  if (!isRecord(entry)) return null;
  const pairs = Object.entries(entry);
  if (pairs.length === 0) return null;
  const [key, def] = pairs[0]!;
  if (def == null) {
    return parseShorthandByField(key, dsByName);
  }
  if (!isRecord(def)) return null;
  if ("entity" in def && "byField" in def) {
    return parseVerboseByField(key, def);
  }
  return null;
};

const findForeignKeyTo = (
  child: DatasourceType,
  parentName: string,
): string | null => {
  for (const field of child.fields) {
    if (field.references === undefined) continue;
    const [refTable] = field.references.split(".");
    if (refTable === parentName) return field.name;
  }
  return null;
};

const buildCandidates = (
  views: ViewType[],
  dsByName: Map<string, DatasourceType>,
): RouteCandidate[] => {
  const out: RouteCandidate[] = [];
  for (const view of views) {
    if (view.name.startsWith("update_") || view.name.startsWith("create_")) {
      continue;
    }
    if (view.kind === "union") {
      out.push({
        name: view.name,
        kind: "view_type",
        inheritsNamespace: "",
        datasourceType: "",
        target: null,
        byFields: [],
      });
      continue;
    }
    const inheritsNamespace =
      view.inherits !== null ? "datasource_types" : "";
    const kind: RouteCandidate["kind"] =
      inheritsNamespace === "datasource_types"
        ? "datasource_type"
        : "view_type";
    const parent = view.inherits !== null ? dsByName.get(view.inherits) : undefined;
    out.push({
      name: view.name,
      kind,
      inheritsNamespace,
      datasourceType: parent?.datasourceType ?? "",
      target: parent?.target ?? null,
      ...(parent?.optimisticConcurrency !== undefined
        ? { optimisticConcurrency: parent.optimisticConcurrency }
        : {}),
      byFields: [],
    });
  }
  return out;
};

const collectCombinedChildNames = (
  routesDoc: RoutesDoc,
  dsByName: Map<string, DatasourceType>,
): Set<string> => {
  const childrenOnly = new Set<string>();
  const parents = new Set<string>();
  for (const [parent] of namedEntries(routesDoc.combined_routes)) {
    parents.add(parent);
  }
  for (const [parentName, defRaw] of namedEntries(routesDoc.combined_routes)) {
    const def = rec(defRaw) as CombinedRouteDef;
    for (const child of def.combined_types ?? []) {
      let childName: string;
      if (typeof child === "string") {
        childName = kebabToSnake(child);
      } else {
        const [rawName, childDef] = Object.entries(child)[0]!;
        if (childDef && (childDef.via || childDef.target)) continue;
        childName = kebabToSnake(rawName);
      }
      if (parents.has(childName)) continue;
      const childDs = dsByName.get(childName);
      if (
        childDs !== undefined &&
        findForeignKeyTo(childDs, parentName) !== null
      ) {
        childrenOnly.add(childName);
      }
    }
  }
  return childrenOnly;
};

const upsertByField = (
  list: RouteByField[],
  parsed: ByFieldParsed,
  dsByName: Map<string, DatasourceType>,
): void => {
  const existing = list.find((e) => e.byField === parsed.byField);
  if (existing) {
    if (existing.methods === undefined || parsed.methods === null) {
      existing.methods = undefined;
    } else if (Array.isArray(parsed.methods)) {
      const union = [...existing.methods];
      for (const m of parsed.methods) {
        if (!union.includes(m)) union.push(m);
      }
      existing.methods = union;
    }
    return;
  }
  const ds = dsByName.get(parsed.entity);
  list.push({
    byField: parsed.byField,
    methods: Array.isArray(parsed.methods) ? parsed.methods : undefined,
    byFieldUnique: ds ? columnIsUnique(ds, parsed.byField) : false,
  });
};

const attachByFields = (
  candidates: RouteCandidate[],
  routesDoc: RoutesDoc,
  dsByName: Map<string, DatasourceType>,
): void => {
  const byFieldByEntity = new Map<string, RouteByField[]>();
  for (const entry of Array.isArray(routesDoc.routes) ? routesDoc.routes : []) {
    const parsed = parseByFieldEntry(entry, dsByName);
    if (parsed === null) continue;
    if (!byFieldByEntity.has(parsed.entity)) {
      byFieldByEntity.set(parsed.entity, []);
    }
    upsertByField(byFieldByEntity.get(parsed.entity)!, parsed, dsByName);
  }
  for (const candidate of candidates) {
    const list = byFieldByEntity.get(candidate.name);
    if (list !== undefined && list.length > 0) {
      candidate.byFields = list;
    }
  }
};

const extractCustomRoutes = (
  routesDoc: RoutesDoc,
  dsByName: Map<string, DatasourceType>,
): CustomRouteEntry[] => {
  const customs: CustomRouteEntry[] = [];
  for (const entry of Array.isArray(routesDoc.routes) ? routesDoc.routes : []) {
    if (!isRecord(entry)) continue;
    if (parseByFieldEntry(entry, dsByName) !== null) continue;
    const [name] = Object.keys(entry);
    if (name === undefined) continue;
    customs.push({ entry, name });
  }
  return customs;
};

const normalizeCombinedChild = (
  child: string | Record<string, CombinedChildDef>,
): NormalizedChild => {
  if (typeof child === "string") {
    return { name: kebabToSnake(child), via: null, target: null, route: null };
  }
  const [rawName, def] = Object.entries(child)[0]!;
  return {
    name: kebabToSnake(rawName),
    via: def && typeof def.via === "string" ? def.via : null,
    target: def && typeof def.target === "string" ? def.target : null,
    route: def && typeof def.route === "string" ? def.route : null,
  };
};

type JunctionMatch = { name: string; parentFk: string; childFk: string };

const detectJunction = (
  parentName: string,
  childName: string,
  dsByName: Map<string, DatasourceType>,
): JunctionMatch | null => {
  const matches: JunctionMatch[] = [];
  for (const [name, def] of dsByName) {
    if (name === parentName || name === childName) continue;
    const parentFk = findForeignKeyTo(def, parentName);
    const childFk = findForeignKeyTo(def, childName);
    if (parentFk !== null && childFk !== null) {
      matches.push({ name, parentFk, childFk });
    }
  }
  if (matches.length === 0) return null;
  if (matches.length > 1) {
    const candidates = matches.map((m) => m.name).join(", ");
    throw new Error(
      `combined_routes: ambiguous junction between "${parentName}" and "${childName}" — candidates: ${candidates}. Add via: to disambiguate.`,
    );
  }
  return matches[0]!;
};

const m2mDescriptor = (
  parentName: string,
  parentBasePath: string,
  args: {
    junction: string;
    target: string;
    parentFkField: string;
    childFkField: string;
    route: string | null;
  },
): M2mDescriptor => {
  const segment = args.route ?? defaultChildSegment(args.target);
  return {
    kind: "m2m",
    parent: parentName,
    parentBasePath,
    parentParam: parentParamName(parentName),
    junction: args.junction,
    target: args.target,
    targetParam: `${snakeToCamel(args.target)}Id`,
    parentFkField: args.parentFkField,
    childFkField: args.childFkField,
    segment,
    segmentTail: segmentTailOf(segment),
  };
};

const directFkDescriptor = (
  parentName: string,
  parentBasePath: string,
  args: { childName: string; fkColumn: string; route: string | null },
): DirectFkDescriptor => {
  const segment = args.route ?? defaultChildSegment(args.childName);
  return {
    kind: "direct-fk",
    parent: parentName,
    parentBasePath,
    parentParam: parentParamName(parentName),
    child: { name: args.childName },
    fkColumn: args.fkColumn,
    segment,
    segmentTail: segmentTailOf(segment),
  };
};

const collectNestedDescriptors = (
  routesDoc: RoutesDoc,
  dsByName: Map<string, DatasourceType>,
): NestedRouteDescriptor[] => {
  const nested: NestedRouteDescriptor[] = [];
  for (const [parentName, defRaw] of namedEntries(routesDoc.combined_routes)) {
    const def = rec(defRaw) as CombinedRouteDef;
    const parentBasePath =
      typeof def.route === "string" && def.route.length > 0
        ? rewriteParentPath(def.route, parentName)
        : defaultParentBasePath(parentName);
    for (const rawChild of def.combined_types ?? []) {
      const child = normalizeCombinedChild(rawChild);
      if (child.via || child.target) {
        const junctionName = child.via;
        const targetName = child.target;
        if (!junctionName || !targetName) {
          throw new Error(
            `combined_routes: M2M child must declare both via: and target: (parent=${parentName}, child=${child.name})`,
          );
        }
        const junctionDef = dsByName.get(junctionName);
        if (junctionDef === undefined) {
          throw new Error(
            `combined_routes: junction "${junctionName}" not found in datasource_types.yaml`,
          );
        }
        const parentFkField = findForeignKeyTo(junctionDef, parentName);
        const childFkField = findForeignKeyTo(junctionDef, targetName);
        if (parentFkField === null || childFkField === null) {
          throw new Error(
            `combined_routes: junction "${junctionName}" missing FK to ${parentName}/${targetName}`,
          );
        }
        nested.push(
          m2mDescriptor(parentName, parentBasePath, {
            junction: junctionName,
            target: targetName,
            parentFkField,
            childFkField,
            route: child.route,
          }),
        );
        continue;
      }
      const childDef = dsByName.get(child.name);
      if (childDef === undefined) {
        throw new Error(
          `combined_routes: child "${child.name}" not found in datasource_types.yaml`,
        );
      }
      const fkColumn = findForeignKeyTo(childDef, parentName);
      if (fkColumn !== null) {
        nested.push(
          directFkDescriptor(parentName, parentBasePath, {
            childName: child.name,
            fkColumn,
            route: child.route,
          }),
        );
        continue;
      }
      const junction = detectJunction(parentName, child.name, dsByName);
      if (junction !== null) {
        nested.push(
          m2mDescriptor(parentName, parentBasePath, {
            junction: junction.name,
            target: child.name,
            parentFkField: junction.parentFk,
            childFkField: junction.childFk,
            route: child.route,
          }),
        );
        continue;
      }
      throw new Error(
        `combined_routes: child "${child.name}" has no FK to parent "${parentName}" and no detectable junction table in datasource_types.yaml`,
      );
    }
  }
  return nested;
};

export const parseRoutes = (args: {
  routesYaml: string;
  views: ViewType[];
  datasources: DatasourceType[];
}): ParsedRoutes => {
  const routesDoc = ensureHealthRouteFirst(parse(args.routesYaml));
  const dsByName = new Map(args.datasources.map((d) => [d.name, d] as const));
  const allCandidates = buildCandidates(args.views, dsByName);
  const childrenOnly = collectCombinedChildNames(routesDoc, dsByName);
  attachByFields(allCandidates, routesDoc, dsByName);
  const customs = extractCustomRoutes(routesDoc, dsByName);
  const nested = collectNestedDescriptors(routesDoc, dsByName);

  const block = routeViewTypeDirective(routesDoc);
  let candidates: RouteCandidate[] = [];
  if (block !== null) {
    const predicate = compileRoutesFilter(block.filter);
    candidates = allCandidates
      .filter((c) => !hasNoRouteSurface(c))
      .filter(predicate)
      .filter((c) => !childrenOnly.has(c.name))
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  return { candidates, customs, nested, childrenOnly, datasources: args.datasources };
};

export const loadRoutes = async (
  reader: {
    read: (name: string) => Promise<string>;
    exists: (name: string) => Promise<boolean>;
  },
  args: { idType: string },
): Promise<ParsedRoutes> => {
  const routesYaml = await reader.read(ROUTES_YAML);
  const hasDs = await reader.exists(DATASOURCE_TYPES_YAML);
  const hasViews = await reader.exists(VIEW_TYPES_YAML);
  const datasourceYaml = hasDs
    ? await reader.read(DATASOURCE_TYPES_YAML)
    : undefined;
  const datasources =
    datasourceYaml !== undefined
      ? parseDatasourceTypes({ yaml: datasourceYaml, idType: args.idType })
      : [];
  const views = hasViews ? await loadViewTypes(reader) : [];
  return parseRoutes({ routesYaml, views, datasources });
};
