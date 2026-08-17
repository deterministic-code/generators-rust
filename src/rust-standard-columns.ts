import { systemColumnsInjectedFor } from "./system-columns.ts";
import type { DatasourceSettings } from "@deterministic-code/generator-sdk/datasource-settings";
import {
  STANDARD_COLUMN_DEFS,
  STANDARD_COLUMN_ORDER,
  dropSystemUuidField,
  type StandardColumn,
} from "@deterministic-code/generator-sdk/codegen/lib/standard-columns";

export {
  STANDARD_COLUMN_DEFS,
  STANDARD_COLUMN_ORDER,
  INLINED_VIEW_AUDIT_FIELDS,
  inlinedViewAuditFieldsExcluding,
} from "@deterministic-code/generator-sdk/codegen/lib/standard-columns";

type SystemFieldsInput = Parameters<typeof systemColumnsInjectedFor>[0];

export function rustInjectedSystemFields(
  def: SystemFieldsInput,
  ds?: DatasourceSettings,
): StandardColumn[] {
  const injected = systemColumnsInjectedFor(def);
  const declaredNames = rawDeclaredNameSet(def);
  const out: StandardColumn[] = [];
  for (const name of STANDARD_COLUMN_ORDER) {
    if (!injected.has(name)) continue;
    if (declaredNames.has(name)) continue;
    out.push({ name, ...STANDARD_COLUMN_DEFS[name] });
  }
  return dropSystemUuidField(out, ds);
}

function rawDeclaredNameSet(def: SystemFieldsInput): Set<string> {
  const out = new Set<string>();
  const fields = Array.isArray(def?.fields) ? def.fields : [];
  for (const entry of fields) {
    if (!entry || typeof entry !== "object") continue;
    const key = Object.keys(entry)[0];
    if (key) out.add(key);
  }
  return out;
}

/** The Rust struct-literal value for the primary-key `id` field under this id_type, matching the struct the datasource-types generator renders via `rustIdType()`: a real `uuid::Uuid`, a `String`, or an `i64`. `variant: "next"` yields a distinct second value so `gets_and_sets` tests aren't tautologies (two `new_v4()`s differ). */
export function rustIdFieldValue(
  ds: DatasourceSettings,
  variant: "sample" | "next" = "sample",
): string {
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
}
