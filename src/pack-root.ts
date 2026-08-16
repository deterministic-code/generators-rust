import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const PACK_ROOT = resolve(here, "..");
export const PACK_TEMPLATES_DIR = resolve(here, "templates");
