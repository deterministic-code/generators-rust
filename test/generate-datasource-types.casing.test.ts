import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { TYPES_YAML } from "../src/specification-parser.ts";
import { generate } from "../src/generate-datasource-types.ts";

const FIXTURE_YAML = `types:
  - notification_type:
      tags: [datasource_type]
      fields:
        - channel_name:
            type: string
`;

const fixtureReader = () =>
  memoryReader({ [TYPES_YAML]: FIXTURE_YAML });

const entryBody = (entry: GenerateEntry): string => {
  if ("contents" in entry) return String(entry.contents);
  return entry.content;
};

const byFilename = async (settings: Record<string, string>) => {
  const map = new Map<string, string>();
  for (const entry of await generate({
    reader: fixtureReader(),
    settings,
  })) {
    map.set(entry.filename, entryBody(entry));
  }
  return map;
};

describe("generate datasource types casing", () => {
  it("Auto uses Camel files, Pascal types, Snake fields", async () => {
    const files = await byFilename({});
    assert.deepEqual([...files.keys()], ["notificationType.rs"]);
    const body = files.get("notificationType.rs")!;
    assert.match(body, /pub struct NotificationType /);
    assert.match(body, /pub channel_name:/);
  });

  it("Snake file names", async () => {
    const files = await byFilename({
      "languages.rust.casing.file_names": "Snake",
    });
    assert.ok(files.has("notification_type.rs"));
  });

  it("Pascal file names", async () => {
    const files = await byFilename({
      "languages.rust.casing.file_names": "Pascal",
    });
    assert.ok(files.has("NotificationType.rs"));
  });

  it("Kebab file names", async () => {
    const files = await byFilename({
      "languages.rust.casing.file_names": "Kebab",
    });
    assert.ok(files.has("notification-type.rs"));
  });

  it("Camel fields", async () => {
    const files = await byFilename({
      "languages.rust.casing.fields": "Camel",
    });
    assert.match(files.get("notificationType.rs")!, /pub channelName:/);
  });

  it("Pascal fields", async () => {
    const files = await byFilename({
      "languages.rust.casing.fields": "Pascal",
    });
    assert.match(files.get("notificationType.rs")!, /pub ChannelName:/);
  });

  it("Snake types", async () => {
    const files = await byFilename({
      "languages.rust.casing.types": "Snake",
    });
    assert.match(files.get("notificationType.rs")!, /pub struct notification_type /);
  });

  it("Kebab directories with Snake files under by-feature", async () => {
    const files = await byFilename({
      "other.organize_by_feature": "true",
      "languages.rust.casing.file_names": "Snake",
      "languages.rust.casing.directories": "Kebab",
    });
    assert.ok(files.has("features/notification-type/notification_type.rs"));
  });
});
