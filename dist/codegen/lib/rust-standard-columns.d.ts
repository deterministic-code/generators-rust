import { systemColumnsInjectedFor } from "../../lib/system-columns.ts";
import type { DatasourceSettings } from "@deterministic-code/generator-sdk/datasource-settings";
import { type StandardColumn } from "@deterministic-code/generator-sdk/codegen/lib/standard-columns";
export { STANDARD_COLUMN_DEFS, STANDARD_COLUMN_ORDER, INLINED_VIEW_AUDIT_FIELDS, inlinedViewAuditFieldsExcluding, } from "@deterministic-code/generator-sdk/codegen/lib/standard-columns";
type SystemFieldsInput = Parameters<typeof systemColumnsInjectedFor>[0];
export declare function rustInjectedSystemFields(def: SystemFieldsInput, ds?: DatasourceSettings): StandardColumn[];
/** The Rust struct-literal value for the primary-key `id` field under this id_type, matching the struct the datasource-types generator renders via `rustIdType()`: a real `uuid::Uuid`, a `String`, or an `i64`. `variant: "next"` yields a distinct second value so `gets_and_sets` tests aren't tautologies (two `new_v4()`s differ). */
export declare function rustIdFieldValue(ds: DatasourceSettings, variant?: "sample" | "next"): string;
