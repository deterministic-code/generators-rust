import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  DeterministicParser,
  SERVICES_YAML,
  type DatasourceType,
  type IDeterministic,
} from "./specification-parser.ts";
import { genericTmpl } from "./resources/service-integration-tests.ts";
import { Emit } from "./emit.ts";

const tableByName = (
  name: string,
  datasources: DatasourceType[],
): DatasourceType | undefined => datasources.find((d) => d.name === name);

const missingIdExpr = (idType: string): string =>
  idType === "uuid"
    ? `"00000000-0000-0000-0000-000000000000"`
    : "99999";

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    const { generics } = deterministic.services;
    const datasources = deterministic.expandedDatasourceTypes;
    return generics
      .filter(
        (c) => tableByName(c.name, datasources)?.datasourceType === "many-to-many",
      )
      .map((c) => {
        const table = tableByName(c.name, datasources);
        const pkType =
          table?.fields.find((f) => f.isPrimaryKey === true)?.type ??
          "integer";
        const withUuid = table?.fields.some((f) => f.name === "uuid") === true;
        return content(
          this.imports.serviceIntegrationTest(c.name),
          fill(genericTmpl, {
            structName: this.casing.serviceClassName(c.name),
            fileBase: this.casing.fileBase(`${c.name}_service`),
            entity: c.name,
            withUuid,
            addPopulatesTest: this.casing.fnIdent(
              withUuid
                ? "add_inserts_a_row_and_auto_populates_id_uuid_created_updated"
                : "add_inserts_a_row_and_auto_populates_id_created_updated",
            ),
            missingId: missingIdExpr(pkType),
          }),
        );
      });
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(SERVICES_YAML);
  const generator = new Generator(ctx.settings);
  return generator.from(
    await DeterministicParser(ctx.reader).parse(ctx.settings, {
      serviceClassName: (entity) => generator.casing.serviceClassName(entity),
    }),
  );
};
