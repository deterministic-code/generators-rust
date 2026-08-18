import {
  camelCase,
  kebabCase,
  pascalCase,
  snakeCase,
} from "change-case";
import pluralize from "pluralize";
import type { SettingsDict } from "./generate-context.ts";
import { settingsBool, settingsStr } from "./settings.ts";

type Convert = (name: string) => string;

const CONVERT: Record<string, Convert> = {
  camel: camelCase,
  pascal: pascalCase,
  snake: snakeCase,
  kebab: kebabCase,
};

const convertFor = (
  settings: SettingsDict,
  key: string,
  fallback: Convert,
): Convert => {
  const raw = settingsStr(settings, key)?.toLowerCase();
  if (!raw || raw === "auto") return fallback;
  return CONVERT[raw] ?? fallback;
};

const pluralSnake = (entity: string): string => {
  const parts = entity.split(/[_-]/);
  parts[parts.length - 1] = pluralize.plural(parts[parts.length - 1]!);
  return parts.join("_");
};

export type ArtifactNaming = {
  byFeature: boolean;
  className: (entity: string) => string;
  fileBase: (entity: string) => string;
  fieldName: (field: string) => string;
  filePath: (entity: string) => string;
};

export const rustNaming = (settings: SettingsDict): ArtifactNaming => {
  const fileCase = convertFor(
    settings,
    "languages.rust.casing.file_names",
    snakeCase,
  );
  const classCase = convertFor(
    settings,
    "languages.rust.casing.types",
    pascalCase,
  );
  const fieldCase = convertFor(
    settings,
    "languages.rust.casing.fields",
    snakeCase,
  );
  const dirCase = convertFor(
    settings,
    "languages.rust.casing.directories",
    snakeCase,
  );
  const byFeature = settingsBool(settings, "other.organize_by_feature");
  const fileBase = (entity: string): string => fileCase(entity);
  return {
    byFeature,
    className: (entity) => classCase(entity),
    fileBase,
    fieldName: (field) => fieldCase(field),
    filePath: (entity) => {
      const file = `${fileBase(entity)}.rs`;
      return byFeature ? `features/${dirCase(entity)}/${file}` : file;
    },
  };
};

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

export type ServiceNaming = {
  byFeature: boolean;
  serviceClassName: (entity: string) => string;
  fileBase: (entity: string) => string;
  filePath: (entity: string) => string;
  casedFileStem: (stem: string) => string;
  customStubPath: (className: string) => string;
  featureEntityFromClass: (className: string) => string;
};

export const rustServiceNaming = (settings: SettingsDict): ServiceNaming => {
  const fileCase = convertFor(
    settings,
    "languages.rust.casing.file_names",
    snakeCase,
  );
  const classCase = convertFor(
    settings,
    "languages.rust.casing.types",
    pascalCase,
  );
  const dirCase = convertFor(
    settings,
    "languages.rust.casing.directories",
    snakeCase,
  );
  const byFeature = settingsBool(settings, "other.organize_by_feature");
  const fileBase = (entity: string): string => fileCase(`${entity}_service`);
  const casedFileStem = (stem: string): string => fileCase(stem);
  return {
    byFeature,
    serviceClassName: (entity) => classCase(`${entity}_service`),
    fileBase,
    filePath: (entity) => {
      const file = `${fileBase(entity)}.rs`;
      return byFeature ? `features/${dirCase(entity)}/${file}` : file;
    },
    casedFileStem,
    featureEntityFromClass,
    customStubPath: (className) => {
      const entity =
        (featureEntityFromClass(className) || "shared").replace(/-/g, "_");
      return `features/${entity}/custom/${casedFileStem(className)}.rs`;
    },
  };
};

export type RouteNaming = {
  byFeature: boolean;
  serviceClassName: (entity: string) => string;
  /** Plural snake stem; under by-feature includes `_router` (SDK bfMarker). */
  fileBase: (entity: string) => string;
  filePath: (entity: string) => string;
  apiPath: (entity: string) => string;
  featureDir: (entity: string) => string;
  serviceModule: (entity: string) => string;
  routeModule: (entity: string) => string;
  serviceUseLine: (entity: string, symbol: string) => string;
  routeModulePath: (entity: string) => string;
  appWiringFilePath: () => string;
};

export const rustRouteNaming = (settings: SettingsDict): RouteNaming => {
  const fileCase = convertFor(
    settings,
    "languages.rust.casing.file_names",
    snakeCase,
  );
  const classCase = convertFor(
    settings,
    "languages.rust.casing.types",
    pascalCase,
  );
  const dirCase = convertFor(
    settings,
    "languages.rust.casing.directories",
    snakeCase,
  );
  const byFeature = settingsBool(settings, "other.organize_by_feature");
  const featureDir = (entity: string): string => dirCase(entity);
  const serviceFileBase = (entity: string): string =>
    fileCase(`${entity}_service`);
  const fileBase = (entity: string): string => {
    const plural = fileCase(pluralSnake(entity));
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
    byFeature,
    serviceClassName: (entity) => classCase(`${entity}_service`),
    fileBase,
    filePath: (entity) => {
      const file = `${fileBase(entity)}.rs`;
      return byFeature ? `features/${featureDir(entity)}/${file}` : file;
    },
    apiPath: (entity) =>
      kebabCase(pluralSnake(entity)).replace(/_/g, "-"),
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
