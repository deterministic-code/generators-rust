import { readFile } from "node:fs/promises";

const resource = (rel: string): Promise<string> =>
  readFile(
    new URL(`../templates/create-view-types/${rel}`, import.meta.url),
    "utf8",
  );

export const typeTmpl = await resource("type.rs.tmpl");
