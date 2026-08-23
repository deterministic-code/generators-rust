import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  datasourceTypesOf,
  isManyToMany,
  pkName,
  tableByName,
} from "@deterministic-code/generators-common/spec-types";
import {
  DeterministicParser,
  SERVICES_YAML,
  type IDeterministic,
  type Type,
} from "./specification-parser.ts";
import { genericTmpl } from "./resources/service-integration-tests.ts";
import { Emit } from "./emit.ts";

const typeByName = (name: string, types: Type[]): Type | undefined =>
  types.find((d) => d.name === name);

const missingIdExpr = (idType: string): string =>
  idType === "uuid"
    ? `"00000000-0000-0000-0000-000000000000"`
    : "99999";

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    const { generics } = deterministic.services;
    const types = datasourceTypesOf(deterministic);
    const tables = tableByName(deterministic);
    return generics
      .filter((c) => {
        const type = typeByName(c.name, types);
        return type !== undefined && isManyToMany(type);
      })
      .map((c) => {
        const type = typeByName(c.name, types);
        const pk =
          type !== undefined ? pkName(type, tables.get(c.name)) : "id";
        const pkType =
          type?.fields.find((f) => f.name === pk)?.type ?? "integer";
        const withUuid = type?.fields.some((f) => f.name === "uuid") === true;
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
