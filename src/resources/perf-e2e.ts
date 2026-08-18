import { readFile } from "node:fs/promises";

export const e2eTmpl = await readFile(
  new URL("../templates/create-perf-e2e/app_perf_client.rs.tmpl", import.meta.url),
  "utf8",
);
