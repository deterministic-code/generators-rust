interface SystemTypeDef {
    datasource_type?: string | null;
    skip_migrations?: boolean;
    fields?: unknown;
}
/** The system columns the migration generator auto-injects for this entity — `id` unless a `primary_key` field is declared, plus `uuid`/`created`/`updated` unless it is a readonly-lookup or carries a custom primary key. Mirrors generate-sql's `generateCreateTable` + `tableHasAuditColumns`. */
export declare function systemColumnsInjectedFor(def: SystemTypeDef): Set<string>;
export {};
