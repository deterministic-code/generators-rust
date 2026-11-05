import {
  createCasingStrategy,
  type ICasingStrategy,
} from "@deterministic-code/generators-common/casing-strategy";
import { Paths } from "@deterministic-code/generators-common/paths";

export const GENERATOR_LANGUAGE = "rust";

const VARIANT_PREFIXES = ["update_", "create_"] as const;

const featureEntity = (entity: string): string => {
  const prefix = VARIANT_PREFIXES.find((p) => entity.startsWith(p));
  return prefix === undefined ? entity : entity.slice(prefix.length);
};

export type PackCasing = ICasingStrategy & {
  entityName: (specName: string) => string
  fileBase: (stem: string) => string
  directory: (entity: string) => string
  filePath: (stem: string) => string
  serviceClassName: (entity: string) => string
  fnIdent: (stem: string) => string
};

/** Language defaults + settings overrides. Layout (by-feature) lives on ImportGenerator. */
export const createCasing = (
  settings: Record<string, string>,
): PackCasing => {
  const casing = createCasingStrategy(GENERATOR_LANGUAGE, settings);
  const paths = new Paths();
  const fileBase = (stem: string): string => casing.convertFileName(stem);
  const entityName = (specName: string): string => casing.convertTypes(paths.typeStem(specName));
  const directory = (entity: string): string =>
    casing.convertDirectories(featureEntity(entity));
  const filePath = (stem: string): string => `${fileBase(stem)}.rs`;
  return {
    convertFileName: (text: string) => casing.convertFileName(text),
    convertTypes: (text: string) => casing.convertTypes(text),
    convertFields: (text: string) => casing.convertFields(text),
    convertDirectories: (text: string) => casing.convertDirectories(text),
    entityName,
    fileBase,
    directory,
    filePath,
    serviceClassName: (entity: string) => casing.convertTypes(`${entity}_service`),
    fnIdent: (stem: string) => casing.convertFields(stem),
  };
};

export const defaultCasing = (
  settings: Record<string, string>,
): ICasingStrategy => createCasing(settings);
