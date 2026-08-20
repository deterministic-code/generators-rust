import {
  CasingFactory,
  LANGUAGE_CASING_DEFAULTS,
  casingOverridesFromSettings,
  type ICasingStrategy,
  type LanguageCasingDefaults,
} from "@deterministic-code/generators-common/casing-strategy";

export const GENERATOR_LANGUAGE = "rust";

export const DEFAULT_CASING: LanguageCasingDefaults =
  LANGUAGE_CASING_DEFAULTS.rust;

const VARIANT_PREFIXES = ["update_", "create_"] as const;

const featureEntity = (entity: string): string => {
  const prefix = VARIANT_PREFIXES.find((p) => entity.startsWith(p));
  return prefix === undefined ? entity : entity.slice(prefix.length);
};

export type PackCasing = ICasingStrategy & {
  byFeature: boolean
  fileBase: (stem: string) => string
  directory: (entity: string) => string
  filePath: (stem: string) => string
  serviceClassName: (entity: string) => string
};

/** Language defaults + settings overrides. Generators call this — not paths.ts. */
export const createCasing = (
  settings: Record<string, string>,
): PackCasing => {
  const casing = CasingFactory.create(
    GENERATOR_LANGUAGE,
    casingOverridesFromSettings(settings, GENERATOR_LANGUAGE),
  );
  const byFeature = settings["other.organize_by_feature"] === "true";
  const fileBase = (stem: string): string => casing.convertFileName(stem);
  const directory = (entity: string): string =>
    casing.convertDirectories(featureEntity(entity));
  const filePath = (stem: string): string => {
    const file = `${fileBase(stem)}.rs`;
    return byFeature ? `features/${directory(stem)}/${file}` : file;
  };
  return {
    convertFileName: (text: string) => casing.convertFileName(text),
    convertTypes: (text: string) => casing.convertTypes(text),
    convertFields: (text: string) => casing.convertFields(text),
    convertDirectories: (text: string) => casing.convertDirectories(text),
    byFeature,
    fileBase,
    directory,
    filePath,
    serviceClassName: (entity: string) => casing.convertTypes(`${entity}_service`),
  };
};

export const defaultCasing = (
  settings: Record<string, string>,
): ICasingStrategy => createCasing(settings);
