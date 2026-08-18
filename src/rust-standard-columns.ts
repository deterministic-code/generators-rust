import { systemColumnsInjectedFor } from "./system-columns.ts";
import type { DatasourceSettings } from "@deterministic-code/generator-sdk/datasource-settings";

export interface StandardColumn {
  name: string;
  type: string;
  isNullable: boolean;
}

export type ColumnDef = Omit<StandardColumn, "name">;

export const STANDARD_COLUMN_DEFS: Record<string, ColumnDef> = {
  id: { type: "number", isNullable: false },
  uuid: { type: "uuid", isNullable: false },
  created: { type: "datetime", isNullable: false },
  updated: { type: "datetime", isNullable: false },
};

export const STANDARD_COLUMN_ORDER: string[] = [
  "id",
  "uuid",
  "created",
  "updated",
];

export const INLINED_VIEW_AUDIT_FIELDS: StandardColumn[] = [
  { name: "id", type: "number", isNullable: false },
  { name: "uuid", type: "string", isNullable: false },
  { name: "created", type: "datetime", isNullable: false },
  { name: "updated", type: "datetime", isNullable: false },
];

export const dropSystemUuidField = (
  fields: StandardColumn[],
  ds?: DatasourceSettings,
): StandardColumn[] =>
  ds && !ds.withUuidColumn
    ? fields.filter((f) => f.name !== "uuid")
    : fields;

export const inlinedViewAuditFieldsExcluding = (
  declaredNames: Iterable<string>,
  ds?: DatasourceSettings,
): StandardColumn[] => {
  const set =
    declaredNames instanceof Set ? declaredNames : new Set(declaredNames);
  return dropSystemUuidField(
    INLINED_VIEW_AUDIT_FIELDS.filter((f) => !set.has(f.name)),
    ds,
  );
};

type SystemFieldsInput = Parameters<typeof systemColumnsInjectedFor>[0];

export const rustInjectedSystemFields = (
  def: SystemFieldsInput,
  ds?: DatasourceSettings,
): StandardColumn[] => {
  const injected = systemColumnsInjectedFor(def);
  const declaredNames = rawDeclaredNameSet(def);
  const out: StandardColumn[] = [];
  for (const name of STANDARD_COLUMN_ORDER) {
    if (!injected.has(name)) continue;
    if (declaredNames.has(name)) continue;
    out.push({ name, ...STANDARD_COLUMN_DEFS[name] });
  }
  return dropSystemUuidField(out, ds);
};

const rawDeclaredNameSet = (def: SystemFieldsInput): Set<string> => {
  const out = new Set<string>();
  const fields = Array.isArray(def?.fields) ? def.fields : [];
  for (const entry of fields) {
    if (!entry || typeof entry !== "object") continue;
    const key = Object.keys(entry)[0];
    if (key) out.add(key);
  }
  return out;
};

/** The Rust struct-literal value for the primary-key `id` field under this id_type, matching the struct the datasource-types generator renders via `rustIdType()`: a real `uuid::Uuid`, a `String`, or an `i64`. `variant: "next"` yields a distinct second value so `gets_and_sets` tests aren't tautologies (two `new_v4()`s differ). */
export const rustIdFieldValue = (
  ds: DatasourceSettings,
  variant: "sample" | "next" = "sample",
): string => {
  switch (ds.rustIdType()) {
    case "uuid::Uuid":
      return "uuid::Uuid::new_v4()";
    case "String":
      return variant === "next"
        ? `String::from("next")`
        : `String::from("sample")`;
    default:
      return variant === "next" ? `42i64` : `1i64`;
  }
};
