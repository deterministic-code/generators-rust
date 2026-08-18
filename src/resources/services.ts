import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-services/${rel}`, import.meta.url),
    "utf8",
  );

export const [genericTmpl, customStubTmpl] = await Promise.all([
  resource("generic.rs.tmpl"),
  resource("custom-stub.rs.tmpl"),
]);
