import { pascalCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { createImportGenerator } from "./import-generator.ts";
import {
  DeterministicParser,
  SERVICES_YAML,
  type ExpandedDatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import { genericTmpl } from "./resources/service-integration-tests.ts";

const serviceClassName = (entity: string): string =>
  pascalCase(`${entity}_service`);

const tableByName = (
  name: string,
  datasources: ExpandedDatasourceType[],
): ExpandedDatasourceType | undefined => datasources.find((d) => d.name === name);

const missingIdExpr = (idType: string): string =>
  idType === "uuid"
    ? `"00000000-0000-0000-0000-000000000000"`
    : "99999";

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const imports = createImportGenerator(".", settings);
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
        imports.serviceIntegrationTest(c.name),
        fill(genericTmpl, {
          structName: serviceClassName(c.name),
          fileBase: `${c.name}_service`,
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
  return generateFrom(
    await DeterministicParser(ctx.reader).parse(ctx.settings, {
      serviceClassName,
    }),
    ctx.settings,
  );
};
