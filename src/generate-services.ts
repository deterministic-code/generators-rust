import { pascalCase, snakeCase } from "change-case";
import { datasourceSettings } from "./common/datasource-settings.ts";
import { fill } from "./common/fill.ts";
import type { GenerateContext, SettingsDict } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import {
  rustServiceNaming,
  type ServiceNaming,
} from "./common/naming.ts";
import {
  loadServices,
  type CustomServiceEntry,
  type ServiceCandidate,
} from "./common/parse-services.ts";
import { customStubTmpl, genericTmpl } from "./resources/services.ts";

const STATUS_OK_DEFAULTS: Record<string, Record<string, string>> = {
  HealthCheckService: {
    check: `Ok(json!({ "status": "ok" }))`,
  },
};

type EmitOptions = {
  naming: ServiceNaming;
};

const emitOptions = (settings: SettingsDict): EmitOptions => ({
  naming: rustServiceNaming(settings),
});

const moduleParts = (mod: string): string[] => {
  const parts = mod.split("/").filter((p) => p !== "" && p !== ".");
  while (parts.length && parts[0] === "..") parts.shift();
  return parts;
};

const structNameFor = (entryName: string): string => {
  if (/^[A-Z]/.test(entryName) && !entryName.includes("_")) return entryName;
  return pascalCase(entryName);
};

const resolveCustomGeneratePath = (
  entry: CustomServiceEntry,
  naming: ServiceNaming,
  byFeature: boolean,
): string => {
  const fileBase = naming.casedFileStem(entry.name);
  const mod = entry.module;

  if (byFeature) {
    if (!mod || !mod.startsWith(".")) return naming.customStubPath(entry.name);
    if (mod.startsWith("./services/") || mod.startsWith("./routes/")) {
      return naming.customStubPath(entry.name);
    }
    const parts = moduleParts(mod);
    if (parts[0] !== "features") {
      const suggestion = naming.customStubPath(entry.name);
      throw new Error(
        `generateCustomServiceStub: service "${entry.name}" has module "${mod}" which is outside ./features/. ` +
          `When organize=by-feature, custom services must live under features/<entity>/custom/. ` +
          `Drop the module: field to use the convention default (${suggestion.replace(/\.rs$/, "")}), ` +
          `or point module: into ./features/.`,
      );
    }
    return `${parts.join("/")}.rs`;
  }

  if (!mod || !mod.startsWith(".")) return `../custom/${fileBase}.rs`;
  const parts = moduleParts(mod);
  if (parts[0] === "services") parts.shift();
  return `../${parts.join("/")}.rs`;
};

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
  const structName = opts.naming.serviceClassName(candidate.name);
  return content(
    opts.naming.filePath(candidate.name),
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
    resolveCustomGeneratePath(entry, opts.naming, opts.naming.byFeature),
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

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const opts = emitOptions(ctx.settings);
  const ds = datasourceSettings(ctx.settings);
  const { generics, customs } = await loadServices(ctx.reader, {
    idType: ds.idType,
    serviceClassName: opts.naming.serviceClassName,
  });
  return [
    ...generics.map((c) => renderGeneric(c, opts)),
    ...customs.map((c) => renderCustom(c, opts)),
  ];
};
