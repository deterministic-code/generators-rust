interface SystemFieldDef {
  primary_key?: boolean;
}

interface SystemTypeDef {
  datasource_type?: string | null;
  skip_migrations?: boolean;
  fields?: unknown;
}

/** Invoke `fn(name, def)` for each well-formed `{ name: def }` field entry, skipping malformed entries. */
function eachField(
  fields: unknown,
  fn: (name: string, def: SystemFieldDef) => void,
): void {
  if (!Array.isArray(fields)) return;
  for (const entry of fields) {
    if (!entry || typeof entry !== "object") continue;
    const pair = Object.entries(entry as Record<string, SystemFieldDef>)[0];
    if (!pair) continue;
    fn(pair[0], pair[1]);
  }
}

/** The system columns the migration generator auto-injects for this entity — `id` unless a `primary_key` field is declared, plus `uuid`/`created`/`updated` unless it is a readonly-lookup or carries a custom primary key. Mirrors generate-sql's `generateCreateTable` + `tableHasAuditColumns`. */
export function systemColumnsInjectedFor(def: SystemTypeDef): Set<string> {
  if (def?.skip_migrations === true) return new Set();
  const fields = Array.isArray(def?.fields) ? def.fields : [];
  let hasAnyPk = false;
  let hasCustomPk = false;
  eachField(fields, (fname, fdef) => {
    if (fdef?.primary_key === true) {
      hasAnyPk = true;
      if (fname !== "id") hasCustomPk = true;
    }
  });
  const injected = new Set<string>();
  if (!hasAnyPk) injected.add("id");
  const isReadonlyLookup = def?.datasource_type === "readonly-lookup";
  if (!isReadonlyLookup && !hasCustomPk) {
    injected.add("uuid");
    injected.add("created");
    injected.add("updated");
  }
  return injected;
}
