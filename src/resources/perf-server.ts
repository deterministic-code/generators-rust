import { readFile } from "node:fs/promises";

export const serverTmpl = await readFile(
  new URL("../templates/create-perf-server/perf_server.rs.tmpl", import.meta.url),
  "utf8",
);
