import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-openapi/${rel}`, import.meta.url),
    "utf8",
  );

export const [routerTmpl, conformanceTmpl] = await Promise.all([
  resource("openapi.rs.tmpl"),
  resource("openapi_conformance.rs.tmpl"),
]);
