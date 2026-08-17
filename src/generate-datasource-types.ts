import { datasourceSettings } from "./common/datasource-settings.ts";
import type { IDeterministicReader } from "./common/deterministic-reader.ts";
import { commentStyle, renderDocComment } from "./common/doc-comment.ts";
import type { GenerateContext } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import { rustNaming } from "./common/naming.ts";
import { settingsStr } from "./common/settings.ts";
import { convertSpecType } from "./common/type-converter.ts";
import { systemColumnsInjectedFor } from "./system-columns.ts";

export type { GenerateEntry };

const SYSTEM = {
  id: { type: "number", isNullable: false },
  uuid: { type: "uuid", isNullable: false },
  created: { type: "datetime", isNullable: false },
  updated: { type: "datetime", isNullable: false },
} as const;

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const ds = datasourceSettings(ctx.settings);
  const naming = rustNaming(ctx.settings);
  const schemaVersion =
    settingsStr(ctx.settings, "codegen.schema_version") ?? "1.0";
  const style = commentStyle(settingsStr(ctx.settings, "comments"));
  const tables = await ctx.reader.loadDatasourceTypes(ds.idType);
  return tables.map((table) => {
    const injected = systemColumnsInjectedFor({
      datasource_type: table.datasourceType,
      fields: table.fields.map((f) => ({ [f.name]: {} })),
    });
    const declared = new Set(table.fields.map((f) => f.name));
    const system = (["id", "uuid", "created", "updated"] as const)
      .filter(
        (n) =>
          injected.has(n) &&
          !declared.has(n) &&
          (n !== "uuid" || ds.withUuidColumn),
      )
      .map((name) => ({ name, ...SYSTEM[name] }));
    const fields = [...system, ...table.fields].filter(
      (f) => ds.withUuidColumn || f.name !== "uuid",
    );
    const structName = naming.className(table.name);
    const body = fields
      .map((f) => {
        const t =
          f.name === "id"
            ? ds.rustIdType
            : convertSpecType(f.type, ds.datetimeRepr);
        const wrapped = f.isNullable ? `Option<${t}>` : t;
        return `    pub ${naming.fieldName(f.name)}: ${wrapped},`;
      })
      .join("\n");
    const doc = renderDocComment({
      style,
      summary: `Type ${structName}.`,
      lines: [
        `Datasource type: ${table.datasourceType}.`,
        `Target: StandardCrud.`,
        `Fields: ${fields.length}.`,
      ],
      language: "rust",
    });
    return content(
      naming.filePath(table.name),
      `// schema-version: ${schemaVersion}
${doc}#[derive(Clone, Debug, PartialEq)]
pub struct ${structName} {
${body}
}
`,
    );
  });
};

export const generateDatasourceTypes = async (args: {
  reader: IDeterministicReader;
  settings: GenerateContext["settings"];
}): Promise<GenerateEntry[]> =>
  generate({
    reader: args.reader,
    settings: args.settings,
  });
