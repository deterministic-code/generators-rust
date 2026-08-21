import { pascalCase, snakeCase } from "change-case";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  createImportGenerator,
  type RustImportGenerator,
} from "./import-generator.ts";
import {
  DeterministicParser,
  SERVICES_YAML,
  type CustomServiceEntry,
  type ServiceCandidate,
  type IDeterministic,
} from "./specification-parser.ts";
import { customStubTmpl, genericTmpl } from "./resources/services.ts";

const STATUS_OK_DEFAULTS: Record<string, Record<string, string>> = {
  HealthCheckService: {
    check: `Ok(json!({ "status": "ok" }))`,
  },
};

type EmitOptions = {
  imports: RustImportGenerator;
};

const emitOptions = (settings: Record<string, string>): EmitOptions => ({
  imports: createImportGenerator(".", settings),
});

const structNameFor = (entryName: string): string => {
  if (/^[A-Z]/.test(entryName) && !entryName.includes("_")) return entryName;
  return pascalCase(entryName);
};

const serviceClassName = (entity: string): string =>
  pascalCase(`${entity}_service`);

const defaultBodyFor = (serviceName: string, method: string): string => {
  const known = STATUS_OK_DEFAULTS[serviceName]?.[method];
  if (known !== undefined) return known;
  return `Ok(serde_json::json!({}))`;
};

const useJsonMacro = (serviceName: string): boolean =>
  Object.prototype.hasOwnProperty.call(STATUS_OK_DEFAULTS, serviceName);

const renderGeneric = (
  candidate: ServiceCandidate,
  opts: EmitOptions,
): GenerateEntry => {
  const structName = serviceClassName(candidate.name);
  return content(
    opts.imports.service(candidate.name),
    fill(genericTmpl, {
      structName,
      entitySnakeLiteral: JSON.stringify(candidate.name),
    }),
  );
};

const renderCustom = (
  entry: CustomServiceEntry,
  opts: EmitOptions,
): GenerateEntry => {
  const structName = structNameFor(entry.name);
  const methods = entry.methods.map((method) => ({
    rustFn: snakeCase(method),
    body: defaultBodyFor(entry.name, method),
  }));
  return content(
    opts.imports.serviceCustom(entry.name, entry.module),
    fill(customStubTmpl, {
      structName,
      useJsonMacro: useJsonMacro(entry.name),
      hasMethods: methods.length > 0,
      methods,
      invokeArms: entry.methods.map((method) => ({
        method,
        rustFn: snakeCase(method),
      })),
      argsParam: methods.length === 0 ? "_args" : "args",
    }),
  );
};

const generateFrom = (
  deterministic: IDeterministic,
  settings: Record<string, string>,
): GenerateEntry[] => {
  const opts = emitOptions(settings);
  const { generics, customs } = deterministic.services;
  return [
    ...generics.map((c) => renderGeneric(c, opts)),
    ...customs.map((c) => renderCustom(c, opts)),
  ];
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
