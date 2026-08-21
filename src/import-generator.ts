import { posix } from "node:path";
import { kebabCase, pascalCase, snakeCase } from "change-case";
import pluralize from "pluralize";
import type { IImportGenerator } from "@deterministic-code/generators-common/import-generator";
import { createCasing, type PackCasing } from "./common/default-casing.ts";

const VARIANT_PREFIXES = ["update_", "create_"] as const;

const featureEntity = (entity: string): string => {
  const prefix = VARIANT_PREFIXES.find((p) => entity.startsWith(p));
  return prefix === undefined ? entity : entity.slice(prefix.length);
};

const pluralSnake = (entity: string): string => {
  const parts = entity.split(/[_-]/);
  parts[parts.length - 1] = pluralize.plural(parts[parts.length - 1]!);
  return parts.join("_");
};

const CUSTOM_SUFFIX_TOKENS = new Set(["service", "route"]);

/** HealthCheckService → health-check; bare "Service" → "". */
const featureEntityFromClass = (className: string): string => {
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

const modulePathParts = (mod: string): string[] => {
  const parts = mod.split("/").filter((p) => p !== "" && p !== ".");
  while (parts.length && parts[0] === "..") parts.shift();
  return parts;
};

const crateFromFile = (toFile: string): string => {
  const stem = toFile.endsWith(".rs") ? toFile.slice(0, -3) : toFile;
  return `crate::${stem.split("/").join("::")}`;
};

export class RustImportGenerator implements IImportGenerator {
  private readonly organizeByFeature: boolean;
  private readonly flat: boolean;
  private readonly basePath: string;
  private readonly casing: PackCasing;

  constructor(basePath: string, settings: Record<string, string>) {
    this.basePath = basePath;
    this.flat = basePath !== "" && basePath !== ".";
    this.organizeByFeature =
      !this.flat && settings["other.organize_by_feature"] === "true";
    this.casing = createCasing(settings);
  }

  datasource(entity: string): string {
    return this.cased(this.underBase(this.featureFile(entity, entity)), entity);
  }

  datasourceRel(entity: string): string {
    return this.rel("types/generated/datasource", this.datasource(entity));
  }

  datasourceQual(entity: string): string {
    return this.typeQual(this.datasourceRel(entity), pascalCase(entity));
  }

  datasourceValidator(entity: string): string {
    if (this.organizeByFeature) {
      return this.underBase(`features/${entity}/${entity}_validator.rs`);
    }
    return this.underBase(`datasource_${entity}_validator.rs`);
  }

  datasourceValidatorRel(entity: string): string {
    return this.rel(
      "types/generated/datasource/validators",
      this.datasourceValidator(entity),
    );
  }

  view(entity: string): string {
    return this.underBase(this.featureFile(entity, entity, featureEntity(entity)));
  }

  viewRel(entity: string): string {
    return this.rel("types/generated/views", this.view(entity));
  }

  viewQual(entity: string): string {
    return this.typeQual(this.viewRel(entity), pascalCase(entity));
  }

  viewValidator(entity: string): string {
    if (this.organizeByFeature) {
      return this.underBase(
        `features/${featureEntity(entity)}/${entity}_validator.rs`,
      );
    }
    return this.underBase(`${entity}_validator.rs`);
  }

  viewValidatorRel(entity: string): string {
    return this.rel(
      "types/generated/views/validators",
      this.viewValidator(entity),
    );
  }

  service(entity: string): string {
    return this.underBase(this.featureFile(entity, `${entity}_service`));
  }

  serviceRel(entity: string): string {
    return this.rel("services/generated", this.service(entity));
  }

  serviceCustom(name: string, module?: string): string {
    return this.resolveCustom(name, module, "services");
  }

  serviceCustomRel(entity: string): string {
    const stem = `${entity}_service`;
    return this.organizeByFeature
      ? `features/${entity}/custom/${stem}.rs`
      : `services/custom/${stem}.rs`;
  }

  serviceTest(entity: string): string {
    return this.test(this.service(entity), `${entity}_service`);
  }

  serviceTestRel(entity: string): string {
    return this.rel("services/generated/__tests__", this.serviceTest(entity));
  }

  serviceIntegrationTest(entity: string): string {
    const file = `${entity}_service_integration_tests.rs`;
    if (this.organizeByFeature) {
      return `features/${entity}/__tests__/${file}`;
    }
    return file;
  }

  serviceIntegrationTestRel(entity: string): string {
    return this.rel(
      "services/generated/__tests__",
      this.serviceIntegrationTest(entity),
    );
  }

  serviceUse(entity: string, symbol: string): string {
    return `use ${this.spec("", this.serviceRel(entity))}::${symbol};`;
  }

  route(entity: string): string {
    return this.underBase(this.featureFile(entity, this.routeStem(entity)));
  }

  routeRel(entity: string): string {
    return this.rel("routes/generated", this.route(entity));
  }

  routeCustom(name: string, module?: string): string {
    return this.resolveCustom(name, module, "routes");
  }

  routeTest(entity: string): string {
    return this.test(this.route(entity), this.routeStem(entity));
  }

  enrichment(_targetTable: string): string {
    return "";
  }

  test(srcFile: string, fileBase: string): string {
    const file = `${fileBase}_tests.rs`;
    if (this.organizeByFeature) {
      return `${posix.dirname(srcFile)}/__tests__/${file}`;
    }
    return file;
  }

  testSpec(srcFile: string, fileBase: string): string {
    return this.spec(this.test(srcFile, fileBase), srcFile);
  }

  index(beside: string): string {
    if (this.organizeByFeature) return "";
    return posix.join(posix.dirname(beside), "mod.rs");
  }

  spec(_fromFile: string, toFile: string): string {
    return crateFromFile(toFile);
  }

  routeModule(entity: string): string {
    return this.routeStem(entity);
  }

  appWiring(): string {
    return this.organizeByFeature ? "features/app_wiring.rs" : "app_wiring.rs";
  }

  validatorFn(
    kind: "datasource" | "view",
    entity: string,
    fn: string,
  ): string {
    if (this.organizeByFeature) {
      const file =
        kind === "datasource"
          ? this.datasourceValidator(entity)
          : this.viewValidator(entity);
      return `${this.spec("", file)}::${fn}`;
    }
    const ns =
      kind === "datasource"
        ? "crate::types::generated::datasource::validators"
        : "crate::types::generated::views::validators";
    return `${ns}::${fn}`;
  }

  apiPath(entity: string): string {
    return kebabCase(pluralSnake(entity)).replace(/_/g, "-");
  }

  private rel(prefix: string, file: string): string {
    if (this.organizeByFeature || this.flat) return file;
    return `${prefix}/${file}`;
  }

  private typeQual(toFile: string, symbol: string): string {
    const crate = this.spec("", toFile);
    if (this.organizeByFeature) return `${crate}::${symbol}`;
    const parts = crate.split("::");
    parts.pop();
    return `${parts.join("::")}::${symbol}`;
  }

  private cased(laid: string, entity: string): string {
    const identityFile = `${entity}.rs`;
    const casedFile = `${this.casing.fileBase(entity)}.rs`;
    const withFile = laid.endsWith(identityFile)
      ? laid.slice(0, -identityFile.length) + casedFile
      : laid;
    const identityDir = `/${entity}/`;
    const casedDir = `/${this.casing.directory(entity)}/`;
    return withFile.includes(identityDir)
      ? withFile.replace(identityDir, casedDir)
      : withFile;
  }

  private routeStem(entity: string): string {
    const plural = pluralSnake(entity);
    return this.organizeByFeature ? `${plural}_router` : plural;
  }

  private underBase(file: string): string {
    if (!this.flat) return file;
    return `${this.basePath}/${file}`;
  }

  private featureFile(entity: string, stem: string, dir = entity): string {
    const file = `${stem}.rs`;
    return this.organizeByFeature ? `features/${dir}/${file}` : file;
  }

  private resolveCustom(
    name: string,
    mod: string | undefined,
    layer: "services" | "routes",
  ): string {
    const kind = layer === "services" ? "service" : "route";
    const stubFn =
      layer === "services"
        ? "generateCustomServiceStub"
        : "generateCustomRouteStub";
    const fileBase = snakeCase(name);
    const entity = (
      featureEntityFromClass(name) || "shared"
    ).replace(/-/g, "_");
    const defaultStub = this.organizeByFeature
      ? `features/${entity}/custom/${fileBase}.rs`
      : `../custom/${fileBase}.rs`;
    if (this.organizeByFeature) {
      if (
        !mod ||
        !mod.startsWith(".") ||
        mod.startsWith("./services/") ||
        mod.startsWith("./routes/")
      ) {
        return defaultStub;
      }
      const parts = modulePathParts(mod);
      if (parts[0] !== "features") {
        throw new Error(
          `${stubFn}: ${kind} "${name}" has module "${mod}" which is outside ./features/. ` +
            `When organize=by-feature, custom ${layer} must live under features/<entity>/custom/. ` +
            `Drop the module: field to use the convention default (${defaultStub.replace(/\.rs$/, "")}), ` +
            `or point module: into ./features/.`,
        );
      }
      return `${parts.join("/")}.rs`;
    }
    if (!mod || !mod.startsWith(".")) return defaultStub;
    const parts = modulePathParts(mod);
    if (parts[0] === layer) parts.shift();
    return `../${parts.join("/")}.rs`;
  }
}

export const createImportGenerator = (
  basePath: string,
  settings: Record<string, string>,
): RustImportGenerator => new RustImportGenerator(basePath, settings);
