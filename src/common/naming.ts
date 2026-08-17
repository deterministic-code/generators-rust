import {
  camelCase,
  kebabCase,
  pascalCase,
  snakeCase,
} from "change-case";
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
