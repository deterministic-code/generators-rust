import { readFile } from "node:fs/promises";

export const [routerTmpl, conformanceTmpl] = await Promise.all([
  readFile(
    new URL("../templates/create-openapi/openapi.rs.tmpl", import.meta.url),
    "utf8",
  ),
  readFile(
    new URL(
      "../templates/create-openapi/openapi_conformance.rs.tmpl",
      import.meta.url,
    ),
    "utf8",
  ),
]);
