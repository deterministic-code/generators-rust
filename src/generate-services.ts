import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  DeterministicParser,
  SERVICES_YAML,
  type CustomServiceEntry,
  type ServiceCandidate,
  type IDeterministic,
} from "./specification-parser.ts";
import { customStubTmpl, genericTmpl } from "./resources/services.ts";
import { Emit } from "./emit.ts";

const STATUS_OK_DEFAULTS: Record<string, Record<string, string>> = {
  HealthCheckService: {
    check: `Ok(json!({ "status": "ok" }))`,
  },
};

const defaultBodyFor = (serviceName: string, method: string): string => {
  const known = STATUS_OK_DEFAULTS[serviceName]?.[method];
  if (known !== undefined) return known;
  return `Ok(serde_json::json!({}))`;
};

const useJsonMacro = (serviceName: string): boolean =>
  Object.prototype.hasOwnProperty.call(STATUS_OK_DEFAULTS, serviceName);

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    const { generics, customs } = deterministic.services;
    return [
      ...generics.map((c) => this.generic(c)),
      ...customs.map((c) => this.custom(c)),
    ];
  }

  private generic(candidate: ServiceCandidate): GenerateEntry {
    const structName = this.casing.serviceClassName(candidate.name);
    return content(
      this.imports.service(candidate.name),
      fill(genericTmpl, {
        structName,
        entitySnakeLiteral: JSON.stringify(candidate.name),
      }),
    );
  }

  private custom(entry: CustomServiceEntry): GenerateEntry {
    const structName = this.casing.convertTypes(entry.name);
    const methods = entry.methods.map((method) => ({
      rustFn: this.casing.convertFields(method),
      body: defaultBodyFor(entry.name, method),
    }));
    return content(
      this.imports.serviceCustom(entry.name, entry.module),
      fill(customStubTmpl, {
        structName,
        useJsonMacro: useJsonMacro(entry.name),
        hasMethods: methods.length > 0,
        methods,
        invokeArms: entry.methods.map((method) => ({
          method,
          rustFn: this.casing.convertFields(method),
        })),
        argsParam: methods.length === 0 ? "_args" : "args",
      }),
    );
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
