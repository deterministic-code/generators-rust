import { DEFAULT_COMMENT_STYLE } from "@deterministic-code/generator-sdk/generate-doc-comment";
import { datasourceTypesGenerator } from "@deterministic-code/generator-sdk/codegen-context";
import { rustInjectedSystemFields } from "./rust-standard-columns.ts";
import { datasourceTypesModule } from "@deterministic-code/generator-sdk/codegen/lib/generate-settings-options";
import { createTypeMapper } from "@deterministic-code/generator-sdk/codegen/lib/type-mapper";
import { datasourceSettingsFor } from "@deterministic-code/generator-sdk/codegen/lib/ts-datasource-settings";
import { datasourceTypeDoc } from "@deterministic-code/generator-sdk/codegen/lib/datasource-types-generate-types";
import type {
  DatasourceField,
  GenerateCtx,
  GeneratedFile,
  NormalizedTable,
} from "@deterministic-code/generator-sdk/codegen/lib/datasource-types-generate-types";

interface RawFieldDef {
  type: string;
  is_nullable?: boolean;
}

interface RawTableDef {
  datasource_type?: string;
  fields: Array<Record<string, RawFieldDef>>;
}

interface RustGenerateOptions {
  baseClass: null;
  schemaVersion: string;
  style: unknown;
  idType?: string;
  datetime?: string;
  withUuidColumn?: boolean;
}

type RustCtx = GenerateCtx<RustGenerateOptions>;

export const DEFAULT_GENERATE_OPTIONS: RustGenerateOptions = {
  baseClass: null,
  schemaVersion: "1.0",
  style: DEFAULT_COMMENT_STYLE,
};

export const mapRustType = createTypeMapper("rust");

/** The rust type for an id column. Delegates the `settings.datasource.id_type` cases to the shared `DatasourceSettings` owner; the leading guard passes a raw `i32` rust type through (the SDK owner only knows settings id_types, not rust primitives). */
export function normalizeIdType(idType: string | undefined): string {
  if (idType === "i32") return "i32";
  return datasourceSettingsFor({ idType }).rustIdType();
}

function mapType(
  field: DatasourceField,
  idType: string | undefined,
  datetime: string | undefined,
): string {
  if (field.name === "id") return normalizeIdType(idType);
  return mapRustType(field.type, { datetime });
}

function normalizeTable(entry: Record<string, RawTableDef>): NormalizedTable {
  const [name, def] = Object.entries(entry)[0];
  const declared: DatasourceField[] = def.fields.map((f) => {
    const [fname, fdef] = Object.entries(f)[0];
    return {
      name: fname,
      type: fdef.type,
      isNullable: fdef.is_nullable === true,
    };
  });
  return {
    name,
    datasourceType: def.datasource_type,
    fields: [...rustInjectedSystemFields(def), ...declared],
  };
}

function generateField(field: DatasourceField, ctx: RustCtx): string {
  const rustType = mapType(field, ctx.opts.idType, ctx.opts.datetime);
  const wrapped = field.isNullable ? `Option<${rustType}>` : rustType;
  return `    pub ${ctx.fields.name(field.name)}: ${wrapped},`;
}

function renderTable(table: NormalizedTable, ctx: RustCtx): GeneratedFile {
  const { names, opts, layout } = ctx;
  const structName = names.className(table.name);
  const path = layout.filePath(table.name, "datasource-type");
  const withUuidColumn =
    datasourceSettingsFor(opts).withUuidColumn && opts.withUuidColumn;
  const fields = withUuidColumn
    ? table.fields
    : table.fields.filter((f) => f.name !== "uuid");
  const body = fields.map((f) => generateField(f, ctx)).join("\n");

  const doc = datasourceTypeDoc({
    className: structName,
    datasourceType: table.datasourceType,
    fieldCount: fields.length,
    style: opts.style,
    language: "rust",
  });

  const content = `// schema-version: ${opts.schemaVersion}
${doc}#[derive(Clone, Debug, PartialEq)]
pub struct ${structName} {
${body}
}
`;
  return { path, content };
}

const baseGenerate = datasourceTypesGenerator(normalizeTable, renderTable)();

export const { render, createGenerator, generate } = datasourceTypesModule({
  baseGenerate,
  defaultGenerateOptions: DEFAULT_GENERATE_OPTIONS,
  language: "rust",
});
