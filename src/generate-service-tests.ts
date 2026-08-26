import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  datasourceTypesOf,
  identityColumns,
  tableByName,
} from "@deterministic-code/generators-common/spec-types";
import { DeterministicParser, SERVICES_YAML, type IDeterministic } from "./specification-parser.ts";
import { genericTmpl } from "./resources/service-tests.ts";
import { Emit } from "./emit.ts";

const missingIdExpr = (idType: string): string =>
  idType === "uuid"
    ? `"00000000-0000-0000-0000-000000000000"`
    : "99999";

const missingIdentityJson = (
  type: { fields: Array<{ name: string; type: string }> } | undefined,
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
    const tables = tableByName(deterministic);
    const types = datasourceTypesOf(deterministic);
    return deterministic.services.generics.map((c) => {
      const type = types.find((t) => t.name === c.name);
      const columns =
        type !== undefined ? identityColumns(type, tables.get(c.name)) : ["id"];
      return content(
        this.imports.serviceTest(c.name),
        fill(genericTmpl, {
          structName: this.casing.serviceClassName(c.name),
          fileBase: this.casing.fileBase(`${c.name}_service`),
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
