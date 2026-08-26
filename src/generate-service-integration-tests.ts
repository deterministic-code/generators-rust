import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  datasourceTypesOf,
  identityColumns,
  isManyToMany,
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

const missingIdentityJson = (
  type: Type | undefined,
  columns: string[],
): string => {
  const names = columns.length > 0 ? columns : ["id"];
  const part = (column: string): string => {
    const pkType = type?.fields.find((f) => f.name === column)?.type ?? "integer";
    return missingIdExpr(pkType);
  };
  if (names.length === 1) return part(names[0]!);
  return `{${names.map((n) => `"${n}":${part(n)}`).join(",")}}`;
};

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
        const columns =
          type !== undefined ? identityColumns(type, tables.get(c.name)) : ["id"];
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
            missingId: missingIdentityJson(type, columns),
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
