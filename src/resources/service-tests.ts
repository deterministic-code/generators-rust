import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-service-tests/${rel}`, import.meta.url),
    "utf8",
  );

export const [genericTmpl] = await Promise.all([resource("generic.rs.tmpl")]);
