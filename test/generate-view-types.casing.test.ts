import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { TYPES_YAML } from "../src/specification-parser.ts";
import { generate } from "../src/generate-view-types.ts";

const FIXTURE_YAML = `types:
  - notification_type:
      tags: [view_type]
      fields:
        - channel_name:
            type: string
`;

const entryBody = (entry: GenerateEntry): string => {
  if ("contents" in entry) return String(entry.contents);
  return entry.content;
};

const byFilename = async (settings: Record<string, string>) => {
  const map = new Map<string, string>();
  for (const entry of await generate({
    reader: memoryReader({ [TYPES_YAML]: FIXTURE_YAML }),
    settings,
  })) {
    map.set(entry.filename, entryBody(entry));
  }
  return map;
};

describe("generate view types casing", () => {
  it("Auto uses Camel files, Pascal types, Snake fields", async () => {
    const files = await byFilename({});
    assert.ok(files.has("notificationType.rs"));
    const body = files.get("notificationType.rs")!;
    assert.match(body, /pub struct NotificationType /);
    assert.match(body, /pub channel_name:/);
  });

  it("Pascal file names", async () => {
    const files = await byFilename({
      "languages.rust.casing.file_names": "Pascal",
    });
    assert.ok(files.has("NotificationType.rs"));
  });

  it("Snake file names", async () => {
    const files = await byFilename({
      "languages.rust.casing.file_names": "Snake",
    });
    assert.ok(files.has("notification_type.rs"));
  });

  it("Camel fields", async () => {
    const files = await byFilename({
      "languages.rust.casing.fields": "Camel",
    });
    assert.match(files.get("notificationType.rs")!, /pub channelName:/);
  });

  it("Kebab directories with Pascal files under by-feature", async () => {
    const files = await byFilename({
      "other.organize_by_feature": "true",
      "languages.rust.casing.file_names": "Pascal",
      "languages.rust.casing.directories": "Kebab",
    });
    assert.ok(files.has("features/notification-type/NotificationType.rs"));
  });
});
