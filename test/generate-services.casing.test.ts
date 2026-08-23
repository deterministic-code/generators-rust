import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-services.ts";

const TYPES = `types:
  - notification_type:
      tags: [datasource_type, view_type]
      fields:
        - channel_name:
            type: string
`;

const DATASOURCE = `includes:
  - types:
      filter: tag == "datasource_type"
types:
  - notification_type:
      fields:
        - channel_name:
            is_unique: true
`;

const SERVICES_YAML = `includes:
  - types:
      filter: tag == "view_type"
services: []
`;

const entryBody = (entry: GenerateEntry): string => {
  if ("contents" in entry) return String(entry.contents);
  return entry.content;
};

const byFilename = async (settings: Record<string, string>) => {
  const map = new Map<string, string>();
  for (const entry of await generate({
    reader: memoryReader({
      "types.yaml": TYPES,
      "datasource.yaml": DATASOURCE,
      "services.yaml": SERVICES_YAML,
    }),
    settings,
  })) {
    map.set(entry.filename, entryBody(entry));
  }
  return map;
};

describe("generate services casing", () => {
  it("Auto uses Camel files and Pascal types", async () => {
    const files = await byFilename({});
    assert.ok(files.has("notificationTypeService.rs"));
    const body = files.get("notificationTypeService.rs")!;
    assert.match(body, /pub struct NotificationTypeService /);
  });

  it("Pascal file names", async () => {
    const files = await byFilename({
      "languages.rust.casing.file_names": "Pascal",
    });
    assert.ok(files.has("NotificationTypeService.rs"));
  });

  it("Snake file names", async () => {
    const files = await byFilename({
      "languages.rust.casing.file_names": "Snake",
    });
    assert.ok(files.has("notification_type_service.rs"));
  });

  it("Kebab directories with Pascal files under by-feature", async () => {
    const files = await byFilename({
      "other.organize_by_feature": "true",
      "languages.rust.casing.file_names": "Pascal",
      "languages.rust.casing.directories": "Kebab",
    });
    assert.ok(
      files.has("features/notification-type/NotificationTypeService.rs"),
    );
  });
});
