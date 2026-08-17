import type { IDeterministicReader } from "./deterministic-reader.ts";

/** Flat settings the generate runner passes every lane (dotted keys, string values). */
export type SettingsDict = Record<string, string>;

export type GenerateContext = {
  reader: IDeterministicReader;
  settings: SettingsDict;
};
