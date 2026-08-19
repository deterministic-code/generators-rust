import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-view-type-validators/${rel}`, import.meta.url),
    "utf8",
  );

export const [
  typeTmpl,
  checkArrayNullableTmpl,
  checkArrayTmpl,
  checkNullableTmpl,
  checkRequiredTmpl,
] = await Promise.all([
  resource("type.rs.tmpl"),
  resource("check-array-nullable.rs.tmpl"),
  resource("check-array.rs.tmpl"),
  resource("check-nullable.rs.tmpl"),
  resource("check-required.rs.tmpl"),
]);
