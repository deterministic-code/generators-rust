import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-datasource-types-tests/${rel}`, import.meta.url),
    "utf8",
  );

export const [typeTestTmpl] = await Promise.all([
  resource("type_tests.rs.tmpl"),
]);
