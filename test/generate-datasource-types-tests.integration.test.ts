import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { TYPES_YAML } from "../src/specification-parser.ts";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-datasource-types-tests.ts";

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
        - uuid:
            type: uuid
        - created_at:
            type: datetime
        - nick_name:
            type: string
            is_nullable: true
        - active:
            type: boolean
        - balance:
            type: decimal
        - avatar:
            type: binary
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
  if (entry === undefined) {
    throw new Error(`missing generate entry: ${filename}`);
  }
  return entry;
};

describe("generate datasource types tests", () => {
  const generateWith = (settings: Record<string, string> = {}) =>
    generate({
      reader: fixtureReader(),
      settings,
    });

  const userBody = async (settings: Record<string, string> = {}) => {
    const map = indexEntries(await generateWith(settings));
    const userFile = [...map.keys()].find((name) =>
      name.endsWith("userTests.rs"),
    );
    assert.ok(userFile, "missing userTests.rs generate entry");
    return entryBody(requireEntry(map, userFile));
  };

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

  it("emits one test file per datasource type", async () => {
    const byName = indexEntries(await generateWith({}));
    assert.deepEqual(
      [...byName.keys()].sort(),
      ["roleTests.rs", "userTests.rs"],
    );
  });

  it("imports the generated type from the sibling module", async () => {
    const user = await userBody();
    assert.match(user, /use super::user::\*;/);
    assert.match(user, /fn sample\(\) -> User \{/);
  });

  it("covers getters and setters for inherited id and declared fields", async () => {
    const user = await userBody();
    const fields = [
      "id",
      "uuid",
      "email",
      "role_id",
      "created_at",
      "nick_name",
      "active",
      "balance",
      "avatar",
    ];
    for (const field of fields) {
      assert.match(user, new RegExp(`fn gets_${field}\\(`));
      assert.match(user, new RegExp(`fn sets_${field}\\(`));
    }
    assert.match(user, /fn allows_setting_nick_name_to_none\(/);
    assert.doesNotMatch(user, /fn allows_setting_email_to_none\(/);
    assert.match(
      user,
      /created_at: chrono::DateTime::parse_from_rfc3339\("2024-01-01T00:00:00.000Z"\)/,
    );
    assert.match(user, /email: String::from\("sample"\)/);
    assert.match(user, /active: false,/);
    assert.match(user, /balance: String::from\("0"\)/);
    assert.match(user, /nick_name: Some\(String::from\("sample"\)\)/);
  });

  it("uses uuid ids when datasource.id_type=uuid", async () => {
    const user = await userBody({ "datasource.id_type": "uuid" });
    assert.match(user, /fn gets_id\(/);
    assert.match(user, /fn sets_id\(/);
    assert.match(user, /let initial = String::from\("00000000-0000-0000-0000-000000000000"\);/);
  });

  it("uses i64 ids when datasource.id_type=biginteger", async () => {
    const user = await userBody({ "datasource.id_type": "biginteger" });
    assert.match(user, /id: 1i64,/);
    assert.match(user, /let next = 2i64;/);
    assert.match(user, /fn sample\(\) -> User \{/);
  });

  it("writes codegen.schema_version into the file header", async () => {
    const user = await userBody({ "codegen.schema_version": "9.9" });
    assert.match(user, /schema-version: 9.9/);
  });
});
