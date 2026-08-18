import { parse } from "yaml";
import { referenceFieldShape } from "./datasource-settings.ts";
import { isRecord, namedEntries } from "./yaml-entry.ts";

export type DatasourceField = {
  name: string;
  type: string;
  isNullable: boolean;
  references?: string;
  isPrimaryKey?: boolean;
  isUnique?: boolean;
  minSize?: number;
  size?: number;
  hasDefault?: boolean;
  defaultValue?: unknown;
};

export type DatasourceType = {
  name: string;
  datasourceType: string;
  fields: DatasourceField[];
  /** Single-column unique index field names (from `indexes:`). */
  uniqueIndexFields: string[];
  target?: string | null;
  optimisticConcurrency?: boolean;
};

export const DATASOURCE_TYPES_YAML = "datasource_types.yaml";

type YamlField = {
  type?: string;
  isNullable: boolean;
  references?: string;
  isPrimaryKey: boolean;
  isUnique: boolean;
  minSize?: number;
  size?: number;
  hasDefault: boolean;
  defaultValue?: unknown;
};

type YamlType = {
  datasourceType?: string;
  target?: string | null;
  optimisticConcurrency?: boolean;
  fields: Array<{ name: string } & YamlField>;
  uniqueIndexFields: string[];
};

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const named = (
  value: unknown,
): { name: string; body: unknown } | undefined => {
  if (!isObject(value)) return undefined;
  const name = Object.keys(value)[0];
  return name === undefined ? undefined : { name, body: value[name] };
};

const namedList = (value: unknown): Array<{ name: string; body: unknown }> =>
  Array.isArray(value)
    ? value.flatMap((item) => {
        const entry = named(item);
        return entry === undefined ? [] : [entry];
      })
    : [];

const singleColumnUniqueIndexField = (body: unknown): string | undefined => {
  const raw = isObject(body) ? body : {};
  if (raw.is_unique !== true) return undefined;
  const fields = raw.fields;
  if (!Array.isArray(fields) || fields.length !== 1) return undefined;
  const only = fields[0];
  return typeof only === "string" && only.length > 0 ? only : undefined;
};

const readField = (body: unknown): YamlField => {
  const raw = isObject(body) ? body : {};
  const hasDefault = Object.prototype.hasOwnProperty.call(raw, "default_value");
  return {
    type: typeof raw.type === "string" ? raw.type : undefined,
    isNullable: raw.is_nullable === true,
    references: typeof raw.references === "string" ? raw.references : undefined,
    isPrimaryKey: raw.primary_key === true,
    isUnique: raw.is_unique === true,
    minSize:
      typeof raw.min_size === "number" && Number.isFinite(raw.min_size)
        ? raw.min_size
        : undefined,
    size:
      typeof raw.size === "number" && Number.isFinite(raw.size)
        ? raw.size
        : undefined,
    hasDefault,
    defaultValue: hasDefault ? raw.default_value : undefined,
  };
};

const readType = (body: unknown): YamlType => {
  const raw = isObject(body) ? body : {};
  const uniqueIndexFields: string[] = [];
  for (const [, indexBody] of namedEntries(raw.indexes)) {
    const field = singleColumnUniqueIndexField(indexBody);
    if (field !== undefined && !uniqueIndexFields.includes(field)) {
      uniqueIndexFields.push(field);
    }
  }
  const hasOcc = Object.prototype.hasOwnProperty.call(
    raw,
    "use_optimistic_concurrency",
  );
  return {
    datasourceType:
      typeof raw.datasource_type === "string" ? raw.datasource_type : undefined,
    target: raw.target === null ? null : typeof raw.target === "string" ? raw.target : undefined,
    optimisticConcurrency: hasOcc
      ? raw.use_optimistic_concurrency === true
      : undefined,
    uniqueIndexFields,
    fields: namedList(raw.fields).map(({ name, body }) => ({
      name,
      ...readField(body),
    })),
  };
};

const inheritedType = (
  references: string,
  byName: Map<string, YamlType>,
  idType: string,
): string | undefined => {
  const [parentName, column, extra] = references.split(".");
  if (!parentName || !column || extra !== undefined) {
    return undefined;
  }
  const parent = byName.get(parentName);
  if (parent === undefined) return undefined;
  const pk = parent.fields.find((field) => field.isPrimaryKey);
  if (pk !== undefined) return pk.name === column ? pk.type : undefined;
  return column === "id" ? referenceFieldShape(idType).type : undefined;
};

const fieldType = (
  field: { name: string } & YamlField,
  byName: Map<string, YamlType>,
  idType: string,
): string => {
  if (field.type !== undefined) return field.type;
  if (field.references === undefined) return "string";
  const type = inheritedType(field.references, byName, idType);
  if (type === undefined) {
    throw new Error(
      `invariant: type-less reference "${field.name}" -> "${field.references}" has no resolvable parent primary key`,
    );
  }
  return type;
};

export const parseDatasourceTypes = (args: {
  yaml: string;
  idType: string;
}): DatasourceType[] => {
  const doc: unknown = parse(args.yaml);
  const types = namedList(isObject(doc) ? doc.types : undefined).map(
    ({ name, body }) => ({ name, ...readType(body) }),
  );
  const byName = new Map(types.map((t) => [t.name, t]));
  return types.map((t) => ({
    name: t.name,
    datasourceType: t.datasourceType ?? "standard",
    uniqueIndexFields: t.uniqueIndexFields,
    ...(t.target !== undefined ? { target: t.target } : {}),
    ...(t.optimisticConcurrency !== undefined
      ? { optimisticConcurrency: t.optimisticConcurrency }
      : {}),
    fields: t.fields.map((field) => ({
      name: field.name,
      type: fieldType(field, byName, args.idType),
      isNullable: field.isNullable,
      references: field.references,
      ...(field.isPrimaryKey ? { isPrimaryKey: true } : {}),
      ...(field.isUnique ? { isUnique: true } : {}),
      ...(field.minSize !== undefined ? { minSize: field.minSize } : {}),
      ...(field.size !== undefined ? { size: field.size } : {}),
      ...(field.hasDefault
        ? { hasDefault: true, defaultValue: field.defaultValue }
        : {}),
    })),
  }));
};

/** Unique lookup columns: `is_unique` fields plus single-column unique indexes. */
export const uniqueLookupFields = (
  type: DatasourceType,
): Array<{ field: string; type: string; size?: number }> => {
  const out: Array<{ field: string; type: string; size?: number }> = [];
  const add = (name: string) => {
    if (out.some((e) => e.field === name)) return;
    const f = type.fields.find((x) => x.name === name);
    out.push({
      field: name,
      type: f?.type ?? "string",
      ...(f?.size !== undefined ? { size: f.size } : {}),
    });
  };
  for (const f of type.fields) {
    if (f.isUnique) add(f.name);
  }
  for (const name of type.uniqueIndexFields) add(name);
  return out;
};

export const loadDatasourceTypes = async (
  reader: { read: (name: string) => Promise<string> },
  idType: string,
): Promise<DatasourceType[]> =>
  parseDatasourceTypes({
    yaml: await reader.read(DATASOURCE_TYPES_YAML),
    idType,
  });
