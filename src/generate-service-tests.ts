import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  datasourceTypesOf,
  pkName,
  tableByName,
} from "@deterministic-code/generators-common/spec-types";
import { DeterministicParser, SERVICES_YAML, type IDeterministic } from "./specification-parser.ts";
import { genericTmpl } from "./resources/service-tests.ts";
import { Emit } from "./emit.ts";

const missingIdExpr = (idType: string): string =>
  idType === "uuid"
    ? `"00000000-0000-0000-0000-000000000000"`
    : "99999";

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    const tables = tableByName(deterministic);
    const types = datasourceTypesOf(deterministic);
    return deterministic.services.generics.map((c) => {
      const type = types.find((t) => t.name === c.name);
      const pk = type !== undefined ? pkName(type, tables.get(c.name)) : "id";
      const pkType =
        type?.fields.find((f) => f.name === pk)?.type ?? "integer";
      return content(
        this.imports.serviceTest(c.name),
        fill(genericTmpl, {
          structName: this.casing.serviceClassName(c.name),
          fileBase: this.casing.fileBase(`${c.name}_service`),
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
