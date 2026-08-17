/** The Rust field/binding name for an entity's service — `<table>_service`, used by the app-wiring aggregator. */
export function serviceFieldName(table: string): string {
  return `${table}_service`;
}
