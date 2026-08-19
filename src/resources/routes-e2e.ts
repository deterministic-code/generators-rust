import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-routes-e2e/${rel}`, import.meta.url),
    "utf8",
  );

export const [fileTmpl, setupTmpl, crudTestsTmpl, readonlyTestsTmpl] =
  await Promise.all([
    resource("app_routes_e2e.rs.tmpl"),
    resource("setup.rs.tmpl"),
    resource("crud-tests.rs.tmpl"),
    resource("readonly-tests.rs.tmpl"),
  ]);
