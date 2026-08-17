import { readFile } from "node:fs/promises";
import Mustache from "mustache";

/** Values Mustache interpolates or uses as sections (`{{#fields}}` / `{{#simpleDoc}}`). */
export type FillTokens = Record<string, unknown>;

export const fill = (text: string, tokens: FillTokens): string =>
  Mustache.render(text, tokens, undefined, {
    escape: (value) => String(value),
  });

export const fillFile = async (
  path: string | URL,
  tokens: FillTokens,
): Promise<string> => fill(await readFile(path, "utf8"), tokens);
