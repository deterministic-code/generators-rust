import { parse } from "yaml";
import { referenceFieldShape } from "./datasource-settings.ts";

export type DatasourceField = {
  name: string;
  type: string;
  isNullable: boolean;
  references?: string;
};

export type DatasourceType = {
  name: string;
  datasourceType: string;
  fields: DatasourceField[];
};

export const DATASOURCE_TYPES_YAML = "datasource_types.yaml";

type YamlField = {
  type?: string;
  isNullable: boolean;
  references?: string;
  isPrimaryKey: boolean;
};

type YamlType = {
  datasourceType?: string;
  fields: Array<{ name: string } & YamlField>;
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

const readField = (body: unknown): YamlField => {
  const raw = isObject(body) ? body : {};
  return {
    type: typeof raw.type === "string" ? raw.type : undefined,
    isNullable: raw.is_nullable === true,
    references: typeof raw.references === "string" ? raw.references : undefined,
    isPrimaryKey: raw.primary_key === true,
  };
};

const readType = (body: unknown): YamlType => {
  const raw = isObject(body) ? body : {};
  return {
    datasourceType:
      typeof raw.datasource_type === "string" ? raw.datasource_type : undefined,
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
    fields: t.fields.map((field) => ({
      name: field.name,
      type: fieldType(field, byName, args.idType),
      isNullable: field.isNullable,
      references: field.references,
    })),
  }));
};
