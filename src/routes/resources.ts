import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-routes/${rel}`, import.meta.url),
    "utf8",
  );

export const [
  readonlyPlainTmpl,
  readonlyByFieldsTmpl,
  crudPlainTmpl,
  crudByFieldsTmpl,
  appWiringTmpl,
] = await Promise.all([
  resource("readonly-plain.rs.tmpl"),
  resource("readonly-by-fields.rs.tmpl"),
  resource("crud-plain.rs.tmpl"),
  resource("crud-by-fields.rs.tmpl"),
  resource("app-wiring.rs.tmpl"),
]);
