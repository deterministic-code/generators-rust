import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-routes-tests/${rel}`, import.meta.url),
    "utf8",
  );

export const [
  crudTmpl,
  readonlyTmpl,
  byFieldGetUniqueTmpl,
  byFieldGetListTmpl,
] = await Promise.all([
  resource("crud.rs.tmpl"),
  resource("readonly.rs.tmpl"),
  resource("by-field-get-unique.rs.tmpl"),
  resource("by-field-get-list.rs.tmpl"),
]);
