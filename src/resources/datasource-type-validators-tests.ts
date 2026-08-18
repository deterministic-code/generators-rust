import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(
      `../templates/create-datasource-type-validators-tests/${rel}`,
      import.meta.url,
    ),
    "utf8",
  );

export const typeTestTmpl = await resource("type_tests.rs.tmpl");
