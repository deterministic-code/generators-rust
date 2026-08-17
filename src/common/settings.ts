import type { SettingsDict } from "./generate-context.ts";

export const settingsStr = (
  settings: SettingsDict,
  key: string,
): string | undefined => settings[key];

export const settingsBool = (
  settings: SettingsDict,
  key: string,
): boolean => settings[key] === "true";
