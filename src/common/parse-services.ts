import { parse } from "yaml";
import { compileServicesFilter } from "./compile-filter.ts";
import {
  DATASOURCE_TYPES_YAML,
  parseDatasourceTypes,
  uniqueLookupFields,
  type DatasourceType,
} from "./parse-datasource-types.ts";
import {
  loadViewTypes,
  VIEW_TYPES_YAML,
  type ViewType,
} from "./parse-view-types.ts";
import { isRecord } from "./yaml-entry.ts";

export const SERVICES_YAML = "services.yaml";
export const ROUTES_YAML = "routes.yaml";

export type ServiceByField = {
  field: string;
  type: string;
  size?: number;
};

export type ServiceCandidate = {
  name: string;
  kind: "datasource_type" | "view_type";
  inheritsNamespace: string;
  datasourceType: string | null;
  byFields: ServiceByField[];
};

export type CustomServiceEntry = {
  name: string;
  module?: string;
  methods: string[];
};

export type ParsedServices = {
  generics: ServiceCandidate[];
  customs: CustomServiceEntry[];
};

const HEALTH_SERVICE_NAME = "HealthCheckService";
const HEALTH_SERVICE_MODULE = "./services/custom/health-check-service";

const rec = (value: unknown): Record<string, unknown> =>
  isRecord(value) ? value : {};

const str = (value: unknown): string | undefined =>
  typeof value === "string" ? value : undefined;

type ServiceDirective = { filter?: string };

type RawServiceEntry = { name: string; module?: string };

const ensureHealthServiceFirst = (
  services: RawServiceEntry[],
): RawServiceEntry[] => {
  const seed = {
    name: HEALTH_SERVICE_NAME,
    module: HEALTH_SERVICE_MODULE,
  };
  const idx = services.findIndex((s) => s.name === HEALTH_SERVICE_NAME);
  if (idx === 0) return [...services];
  if (idx > 0) {
    return [
      services[idx]!,
      ...services.slice(0, idx),
      ...services.slice(idx + 1),
    ];
  }
  return [seed, ...services];
};

const isStandardCrudEntry = (entry: RawServiceEntry): boolean =>
  typeof entry.module === "string" &&
  entry.module.startsWith("./services/generated/");

const serviceViewTypeDirective = (
  doc: Record<string, unknown>,
): ServiceDirective | null => {
  const includes = Array.isArray(doc.includes) ? doc.includes : [];
  for (const entry of includes) {
    const raw = rec(entry);
    if (raw.view_type_services !== undefined) {
      const block = rec(raw.view_type_services);
      return { filter: str(block.filter) };
    }
  }
  return null;
};

const buildCandidates = (
  views: ViewType[],
  datasources: DatasourceType[],
): ServiceCandidate[] => {
  const byName = new Map<string, ServiceCandidate>();
  const dsMap = new Map(
    datasources.map((d) => [d.name, d.datasourceType] as const),
  );
  const byFieldsByEntity = new Map(
    datasources.map((d) => [d.name, uniqueLookupFields(d)] as const),
  );

  for (const ds of datasources) {
    byName.set(ds.name, {
      name: ds.name,
      kind: "datasource_type",
      inheritsNamespace: "",
      datasourceType: ds.datasourceType,
      byFields: byFieldsByEntity.get(ds.name) ?? [],
    });
  }

  for (const view of views) {
    if (view.name.startsWith("update_") || view.name.startsWith("create_")) {
      continue;
    }
    if (view.kind === "union") {
      byName.set(view.name, {
        name: view.name,
        kind: "view_type",
        inheritsNamespace: "",
        datasourceType: null,
        byFields: [],
      });
      continue;
    }
    const inheritsNamespace =
      view.inherits !== null ? "datasource_types" : "";
    const datasourceType =
      view.inherits !== null ? (dsMap.get(view.inherits) ?? null) : null;
    const byFields =
      view.inherits !== null
        ? (byFieldsByEntity.get(view.inherits) ?? [])
        : [];
    byName.set(view.name, {
      name: view.name,
      kind: "view_type",
      inheritsNamespace,
      datasourceType,
      byFields,
    });
  }

  return [...byName.values()];
};

type RouteServiceEntry = {
  service: string;
  serviceMethod: string;
};

const isRouteServiceEntry = (
  entry: Record<string, unknown>,
): entry is Record<string, unknown> & RouteServiceEntry =>
  typeof entry.service === "string" && typeof entry.serviceMethod === "string";

const walkRouteServiceEntries = (
  routesDoc: Record<string, unknown>,
  visit: (entry: RouteServiceEntry) => void,
): void => {
  const walk = (value: unknown): void => {
    if (Array.isArray(value)) {
      value.forEach(walk);
      return;
    }
    if (!isRecord(value)) return;
    if (isRouteServiceEntry(value)) visit(value);
    for (const v of Object.values(value)) walk(v);
  };
  walk(routesDoc.routes);
  walk(routesDoc.combined_routes);
};

const collectRouteServiceMethods = (
  routesYaml: string | undefined,
): Map<string, Set<string>> => {
  const byService = new Map<string, Set<string>>();
  if (routesYaml === undefined) return byService;
  const doc = rec(parse(routesYaml));
  walkRouteServiceEntries(doc, (entry) => {
    const set = byService.get(entry.service) ?? new Set<string>();
    set.add(entry.serviceMethod);
    byService.set(entry.service, set);
  });
  return byService;
};

export const parseServices = (args: {
  servicesYaml: string;
  views: ViewType[];
  datasources: DatasourceType[];
  routesYaml?: string;
  /** Class name for entity `e` — used to suppress generics shadowed by custom stubs. */
  serviceClassName: (entity: string) => string;
}): ParsedServices => {
  const doc = rec(parse(args.servicesYaml));
  const rawServices = (Array.isArray(doc.services) ? doc.services : [])
    .flatMap((e): RawServiceEntry[] => {
      const raw = rec(e);
      const name = str(raw.name);
      if (name === undefined) return [];
      return [{ name, module: str(raw.module) }];
    });

  const services = ensureHealthServiceFirst(rawServices);
  const customEntries = services.filter((s) => !isStandardCrudEntry(s));
  const explicitCustomNames = new Set(customEntries.map((s) => s.name));
  const methodsByService = collectRouteServiceMethods(args.routesYaml);

  const block = serviceViewTypeDirective(doc);
  let generics: ServiceCandidate[] = [];
  if (block !== null) {
    const predicate = compileServicesFilter(block.filter);
    generics = buildCandidates(args.views, args.datasources)
      .filter(predicate)
      .filter((c) => !explicitCustomNames.has(args.serviceClassName(c.name)))
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  const customs: CustomServiceEntry[] = customEntries.map((entry) => ({
    name: entry.name,
    module: entry.module,
    methods: [...(methodsByService.get(entry.name) ?? [])].sort(),
  }));

  return { generics, customs };
};

export const loadServices = async (
  reader: {
    read: (name: string) => Promise<string>;
    exists: (name: string) => Promise<boolean>;
  },
  args: {
    idType: string;
    serviceClassName: (entity: string) => string;
  },
): Promise<ParsedServices> => {
  const servicesYaml = await reader.read(SERVICES_YAML);
  const hasDs = await reader.exists(DATASOURCE_TYPES_YAML);
  const hasViews = await reader.exists(VIEW_TYPES_YAML);
  const hasRoutes = await reader.exists(ROUTES_YAML);
  const [datasourceYaml, routesYaml] = await Promise.all([
    hasDs ? reader.read(DATASOURCE_TYPES_YAML) : Promise.resolve(undefined),
    hasRoutes ? reader.read(ROUTES_YAML) : Promise.resolve(undefined),
  ]);
  const datasources =
    datasourceYaml !== undefined
      ? parseDatasourceTypes({ yaml: datasourceYaml, idType: args.idType })
      : [];
  const views = hasViews ? await loadViewTypes(reader) : [];
  return parseServices({
    servicesYaml,
    views,
    datasources,
    routesYaml,
    serviceClassName: args.serviceClassName,
  });
};
