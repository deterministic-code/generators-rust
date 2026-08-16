import { pathToFileURL } from "node:url";

/** Run `fn` only when the module is the process entrypoint (not when imported by a test); print its error to stderr and exit 1. The single home for the import-as-main guard every create-script shared. */
export async function runAsMain(
  importMeta: ImportMeta,
  fn: () => unknown,
): Promise<void> {
  /* c8 ignore start -- entrypoint guard; subprocess tests cover it, v8 doesn't cross spawn() */
  if (
    !process.argv[1] ||
    importMeta.url !== pathToFileURL(process.argv[1]).href
  ) {
    return;
  }
  try {
    await fn();
  } catch (e) {
    process.stderr.write(`${(e as Error).message}\n`);
    process.exit(1);
  }
  /* c8 ignore stop */
}
