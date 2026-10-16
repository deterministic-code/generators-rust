import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { TYPES_YAML } from "../src/specification-parser.ts";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-datasource-types.ts";

const FIXTURE_YAML = `types:
  - user:
      tags: [datasource_type]
      inherits: set
      fields:
        - email:
            type: string
            size: 256
        - role_id:
            type: integer
            references: role.id
  - role:
      tags: [datasource_type]
      inherits: set
      fields:
        - name:
            type: string
`;

const fixtureReader = () =>
  memoryReader({ [TYPES_YAML]: FIXTURE_YAML });

const entryBody = (entry: GenerateEntry): string => {
  if ("contents" in entry) return String(entry.contents);
  return entry.content;
};

const indexEntries = (entries: GenerateEntry[]): Map<string, GenerateEntry> => {
  const map = new Map<string, GenerateEntry>();
  for (const entry of entries) {
    assert.equal(
      map.has(entry.filename),
      false,
      `duplicate generate entry: ${entry.filename}`,
    );
    map.set(entry.filename, entry);
  }
  return map;
};

const requireEntry = (
  map: Map<string, GenerateEntry>,
  filename: string,
): GenerateEntry => {
  const entry = map.get(filename);
  assert.ok(entry, `missing generate entry: ${filename}`);
  return entry;
};

describe("generate", () => {
  it("rejects a missing types.yaml", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({}),
          settings: {},
        }),
      /missing types\.yaml/,
    );
  });

  it("emits one struct file per datasource type", async () => {
    const byName = indexEntries(
      await generate({
        reader: fixtureReader(),
        settings: { application_name: "catalog-api" },
      }),
    );
    assert.deepEqual([...byName.keys()].sort(), ["role.rs", "user.rs"]);
  });

  it("injects inherited id and maps field types", async () => {
    const byName = indexEntries(
      await generate({
        reader: fixtureReader(),
        settings: { application_name: "catalog-api" },
      }),
    );
    const user = entryBody(requireEntry(byName, "user.rs"));
    assert.match(user, /schema-version: 1\.0/);
    assert.match(user, /pub struct User \{/);
    assert.match(user, /pub id: i32,/);
    assert.match(user, /pub email: String,/);
    assert.match(user, /pub role_id: i32,/);
  });

  it("infers FK type from the referenced parent field", async () => {
    const byName = indexEntries(
      await generate({
        reader: memoryReader({
          [TYPES_YAML]: `types:
  - parent:
      tags: [datasource_type]
      inherits: set
      fields: []
  - child:
      tags: [datasource_type]
      inherits: set
      fields:
        - owner_id:
            references: parent.id
`,
        }),
        settings: { application_name: "catalog-api" },
      }),
    );
    const child = entryBody(requireEntry(byName, "child.rs"));
    assert.match(child, /pub owner_id: i32,/);
    assert.doesNotMatch(child, /pub owner_id: String/);
  });

  it("emits Default for non-zero spec defaults and skips language zeros", async () => {
    const byName = indexEntries(
      await generate({
        reader: memoryReader({
          [TYPES_YAML]: `types:
  - flags:
      tags: [datasource_type]
      inherits: set
      fields:
        - is_enabled:
            type: boolean
            default_value: true
        - is_disabled:
            type: boolean
            default_value: false
        - count:
            type: integer
            default_value: 0
        - rank:
            type: integer
            default_value: 1
        - empty_name:
            type: string
            default_value: ""
        - title:
            type: string
            default_value: hello
        - blob:
            type: binary
            default_value: Hex('00')
        - stamped:
            type: datetime
            default_value: UtcNow
        - opted_out:
            type: boolean
            is_nullable: true
            default_value: false
        - token:
            type: uuid
            default_value: NewId
        - price:
            type: decimal
            default_value: "1.50"
`,
        }),
        settings: {},
      }),
    );
    const flags = entryBody(requireEntry(byName, "flags.rs"));
    assert.match(flags, /impl Default for Flags/);
    assert.match(flags, /is_enabled: true,/);
    assert.match(flags, /is_disabled: false,/);
    assert.match(flags, /count: 0,/);
    assert.match(flags, /rank: 1,/);
    assert.match(flags, /empty_name: String::new\(\),/);
    assert.doesNotMatch(flags, /String::from\(""\)/);
    assert.match(flags, /title: String::from\("hello"\),/);
    assert.match(flags, /blob: vec!\[0\],/);
    assert.match(flags, /stamped: chrono::Utc::now\(\),/);
    assert.match(flags, /opted_out: Some\(false\),/);
    assert.match(flags, /token: uuid::Uuid::new_v4\(\)\.to_string\(\),/);
    assert.match(flags, /price: String::from\("1.50"\),/);
  });
});
