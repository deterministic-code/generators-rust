import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { servicePaths, type ServicePaths } from "./common/paths.ts";
import { SpecificationParser } from "./specification-parser.ts";
import { genericTmpl } from "./resources/service-tests.ts";

const testPath = (entity: string, naming: ServicePaths): string => {
  const file = `${naming.fileBase(entity)}_tests.rs`;
  if (!naming.byFeature) return file;
  const typeFile = naming.filePath(entity);
  return `${typeFile.slice(0, typeFile.lastIndexOf("/"))}/__tests__/${file}`;
};

const missingIdExpr = (idType: string): string =>
  idType === "uuid"
    ? `"00000000-0000-0000-0000-000000000000"`
    : "99999";

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const idType = ctx.settings["datasource.id_type"] ?? "integer";
  const naming = servicePaths(ctx.settings);
  const { generics } = await new SpecificationParser(ctx.reader).loadServices({
    idType,
    serviceClassName: naming.serviceClassName,
  });
  return generics.map((c) =>
    content(
      testPath(c.name, naming),
      fill(genericTmpl, {
        structName: naming.serviceClassName(c.name),
        fileBase: naming.fileBase(c.name),
        missingId: missingIdExpr(idType),
      }),
    ),
  );
};
