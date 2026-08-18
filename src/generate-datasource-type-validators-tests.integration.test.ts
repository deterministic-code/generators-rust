import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "./common/deterministic-reader.ts";
import { DATASOURCE_TYPES_YAML } from "./common/parse-datasource-types.ts";
import type { GenerateEntry } from "./common/generate-entry.ts";
import { generate } from "./generate-datasource-type-validators-tests.ts";

const FIXTURE_YAML = `types:
  - user:
      datasource_type: audit
      fields:
        - email:
            type: string
            size: 256
        - role_id:
            references: role.id
        - nick_name:
            type: string
            is_nullable: true
        - token:
            type: uuid
  - role:
      fields:
        - name:
            type: string
`;

const fixtureReader = () =>
  memoryReader({ [DATASOURCE_TYPES_YAML]: FIXTURE_YAML });

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
  if (entry === undefined) {
    throw new Error(`missing generate entry: ${filename}`);
  }
  return entry;
};

describe("generate datasource type validators tests", () => {
  const generateWith = (settings: Record<string, string> = {}) =>
    generate({
      reader: fixtureReader(),
      settings,
    });

  const userBody = async (settings: Record<string, string> = {}) => {
    const map = indexEntries(await generateWith(settings));
    const userFile = [...map.keys()].find((name) =>
      name.endsWith("user_tests.rs"),
    );
    assert.ok(userFile, "missing user_tests.rs generate entry");
    return entryBody(requireEntry(map, userFile));
  };

  it("rejects a missing datasource_types.yaml", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({}),
          settings: {},
        }),
      /missing datasource_types\.yaml/,
    );
  });

  it("emits one validator test file per datasource type", async () => {
    const byName = indexEntries(await generateWith({}));
    assert.deepEqual(
      [...byName.keys()].sort(),
      ["role_tests.rs", "user_tests.rs"],
    );
  });

  it("covers parse, nullable, and invalid uuid cases", async () => {
    const user = await userBody();
    assert.match(user, /fn parses_a_valid_payload/);
    assert.match(user, /fn accepts_none_for_nullable_fields/);
    assert.match(user, /fn rejects_when_invalid_uuid_on_token/);
    assert.match(user, /validate_datasource_user\(&value\)\.is_ok\(\)/);
    assert.match(user, /nick_name: None/);
    assert.match(user, /not-a-uuid/);
  });

  it("writes codegen.schema_version into the file header", async () => {
    const user = await userBody({ "codegen.schema_version": "9.9" });
    assert.match(user, /schema-version: 9.9/);
  });
});
