import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { servicePaths, type ServicePaths } from "./common/paths.ts";
import {
  DeterministicParser,
  SERVICES_YAML,
  type ExpandedDatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import { genericTmpl } from "./resources/service-integration-tests.ts";

const tableByName = (
  name: string,
  datasources: ExpandedDatasourceType[],
): ExpandedDatasourceType | undefined => datasources.find((d) => d.name === name);

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

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const naming = servicePaths(settings);
  const { generics } = deterministic.services;
  const datasources = deterministic.expandedDatasourceTypes;
  return generics
    .filter(
      (c) => tableByName(c.name, datasources)?.datasourceType === "many-to-many",
    )
    .map((c) => {
      const table = tableByName(c.name, datasources);
      const pkType =
        table?.fields.find((f) => f.name === (table.primaryKeyColumn ?? "id"))
          ?.type ?? "integer";
      const withUuid = table?.fields.some((f) => f.name === "uuid") === true;
      return content(
        testPath(c.name, naming),
        fill(genericTmpl, {
          structName: naming.serviceClassName(c.name),
          fileBase: naming.fileBase(c.name),
          entity: c.name,
          withUuid,
          stampColsIdent: withUuid
            ? "id_uuid_created_updated"
            : "id_created_updated",
          missingId: missingIdExpr(pkType),
        }),
      );
    });
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(SERVICES_YAML);
  const naming = servicePaths(ctx.settings);
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings, {
      serviceClassName: naming.serviceClassName,
    }),
    ctx.settings,
  );
};
