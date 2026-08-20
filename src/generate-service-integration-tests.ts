import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { servicePaths, type ServicePaths } from "./common/paths.ts";
import {
  SpecificationParser,
  DATASOURCE_TYPES_YAML,
  type DatasourceType,
} from "./specification-parser.ts";
import { genericTmpl } from "./resources/service-integration-tests.ts";

const tableByName = (
  name: string,
  datasources: DatasourceType[],
): DatasourceType | undefined => datasources.find((d) => d.name === name);

const testPath = (entity: string, naming: ServicePaths): string => {
  const file = `${naming.fileBase(entity)}_integration_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const missingIdExpr = (idType: string): string =>
  idType === "uuid"
    ? `"00000000-0000-0000-0000-000000000000"`
    : "99999";

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const idType = ctx.settings["datasource.id_type"] ?? "integer";
  const naming = servicePaths(ctx.settings);
  const { generics } = await new SpecificationParser(ctx.reader).loadServices({
    idType,
    serviceClassName: naming.serviceClassName,
  });
  const hasDs = await ctx.reader.exists(DATASOURCE_TYPES_YAML);
  const datasources = hasDs
    ? new SpecificationParser().parseDatasourceTypes({
        yaml: await ctx.reader.read(DATASOURCE_TYPES_YAML),
        idType,
      })
    : [];
  const withUuid = idType !== "uuid";
  return generics
    .filter(
      (c) => tableByName(c.name, datasources)?.datasourceType === "many-to-many",
    )
    .map((c) =>
      content(
        testPath(c.name, naming),
        fill(genericTmpl, {
          structName: naming.serviceClassName(c.name),
          fileBase: naming.fileBase(c.name),
          entity: c.name,
          withUuid,
          stampColsIdent: withUuid
            ? "id_uuid_created_updated"
            : "id_created_updated",
          missingId: missingIdExpr(idType),
        }),
      ),
    );
};
