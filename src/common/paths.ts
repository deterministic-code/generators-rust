import { kebabCase, pascalCase, snakeCase } from "change-case";
import pluralize from "pluralize";

const VARIANT_PREFIXES = ["update_", "create_"] as const;

const variantPrefix = (entity: string): string | undefined =>
  VARIANT_PREFIXES.find((p) => entity.startsWith(p));

const featureEntity = (entity: string): string => {
  const prefix = variantPrefix(entity);
  return prefix === undefined ? entity : entity.slice(prefix.length);
};

const organizeByFeature = (settings: Record<string, string>): boolean =>
  settings["other.organize_by_feature"] === "true";

const pluralSnake = (entity: string): string => {
  const parts = entity.split(/[_-]/);
  parts[parts.length - 1] = pluralize.plural(parts[parts.length - 1]!);
  return parts.join("_");
};

export type ArtifactPaths = {
  byFeature: boolean;
  className: (entity: string) => string;
  fileBase: (entity: string) => string;
  fieldName: (field: string) => string;
  filePath: (entity: string) => string;
};

export type ServicePaths = ArtifactPaths & {
  serviceClassName: (entity: string) => string;
  casedFileStem: (stem: string) => string;
  customStubPath: (className: string) => string;
};

export type RoutePaths = ArtifactPaths & {
  serviceClassName: (entity: string) => string;
  apiPath: (entity: string) => string;
  featureDir: (entity: string) => string;
  serviceModule: (entity: string) => string;
  routeModule: (entity: string) => string;
  serviceUseLine: (entity: string, symbol: string) => string;
  routeModulePath: (entity: string) => string;
  appWiringFilePath: () => string;
};

const core = (
  byFeature: boolean,
  fileBase: (entity: string) => string,
): Pick<
  ArtifactPaths,
  "byFeature" | "className" | "fileBase" | "fieldName"
> => ({
  byFeature,
  className: (entity) => pascalCase(entity),
  fileBase,
  fieldName: (field) => field,
});

const CUSTOM_SUFFIX_TOKENS = new Set(["service", "route"]);

/** HealthCheckService → health-check; bare "Service" → "". */
export const featureEntityFromClass = (className: string): string => {
  const tokens = className
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1-$2")
    .replace(/[_\s]+/g, "-")
    .toLowerCase()
    .split("-")
    .filter(Boolean);
  if (tokens.length === 0) return "";
  const last = tokens[tokens.length - 1];
  if (tokens.length > 1 && CUSTOM_SUFFIX_TOKENS.has(last)) tokens.pop();
  return tokens.join("-");
};

export const datasourcePaths = (
  settings: Record<string, string>,
): ArtifactPaths => {
  const byFeature = organizeByFeature(settings);
  const fileBase = (entity: string) => entity;
  const filePath = (entity: string) => {
    const file = `${fileBase(entity)}.rs`;
    return byFeature ? `features/${entity}/${file}` : file;
  };
  return { ...core(byFeature, fileBase), filePath };
};

export const viewPaths = (settings: Record<string, string>): ArtifactPaths => {
  const byFeature = organizeByFeature(settings);
  const fileBase = (entity: string) => entity;
  const filePath = (entity: string) => {
    const file = `${fileBase(entity)}.rs`;
    return byFeature
      ? `features/${featureEntity(entity)}/${file}`
      : file;
  };
  return { ...core(byFeature, fileBase), filePath };
};

export const servicePaths = (settings: Record<string, string>): ServicePaths => {
  const byFeature = organizeByFeature(settings);
  const fileBase = (entity: string) => `${entity}_service`;
  const casedFileStem = (stem: string) => snakeCase(stem);
  return {
    ...core(byFeature, fileBase),
    filePath: (entity) => {
      const file = `${fileBase(entity)}.rs`;
      return byFeature ? `features/${entity}/${file}` : file;
    },
    serviceClassName: (entity) => pascalCase(`${entity}_service`),
    casedFileStem,
    customStubPath: (className) => {
      const entity = (
        featureEntityFromClass(className) || "shared"
      ).replace(/-/g, "_");
      return `features/${entity}/custom/${casedFileStem(className)}.rs`;
    },
  };
};

export const routePaths = (settings: Record<string, string>): RoutePaths => {
  const byFeature = organizeByFeature(settings);
  const featureDir = (entity: string): string => entity;
  const serviceFileBase = (entity: string): string => `${entity}_service`;
  const fileBase = (entity: string): string => {
    const plural = pluralSnake(entity);
    return byFeature ? `${plural}_router` : plural;
  };
  const serviceModule = (entity: string): string => {
    const mod = serviceFileBase(entity);
    return byFeature
      ? `crate::features::${featureDir(entity)}::${mod}`
      : `crate::services::generated::${mod}`;
  };
  const routeModule = (entity: string): string => {
    const mod = fileBase(entity);
    return byFeature
      ? `crate::features::${featureDir(entity)}::${mod}`
      : `crate::routes::generated::${mod}`;
  };
  return {
    ...core(byFeature, fileBase),
    filePath: (entity) => {
      const file = `${fileBase(entity)}.rs`;
      return byFeature ? `features/${featureDir(entity)}/${file}` : file;
    },
    serviceClassName: (entity) => pascalCase(`${entity}_service`),
    apiPath: (entity) => kebabCase(pluralSnake(entity)).replace(/_/g, "-"),
    featureDir,
    serviceModule,
    routeModule,
    serviceUseLine: (entity, symbol) =>
      `use ${serviceModule(entity)}::${symbol};`,
    routeModulePath: (entity) => routeModule(entity),
    appWiringFilePath: () =>
      byFeature ? "features/app_wiring.rs" : "app_wiring.rs",
  };
};
