import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { TYPES_YAML } from "../src/specification-parser.ts";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-datasource-type-validators.ts";

const FIXTURE_YAML = `types:
  - user:
      tags: [datasource_type]
      inherits: set
      fields:
        - email:
            type: string
            size: 256
            min_size: 3
        - role_id:
            type: integer
            references: role.id
        - nick_name:
            type: string
            is_nullable: true
        - score:
            type: float
            min_size: 0
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

describe("generate datasource type validators", () => {
  const generateWith = (settings: Record<string, string> = {}) =>
    generate({
      reader: fixtureReader(),
      settings,
    });

  const userBody = async (settings: Record<string, string> = {}) => {
    const map = indexEntries(await generateWith(settings));
    const userFile = [...map.keys()].find((name) =>
      name.endsWith("datasourceUserValidator.rs"),
    );
    assert.ok(userFile, "missing datasourceUserValidator.rs generate entry");
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

  it("emits one validator file per datasource type", async () => {
    const byName = indexEntries(await generateWith({}));
    assert.deepEqual(
      [...byName.keys()].sort(),
      ["datasourceRoleValidator.rs", "datasourceUserValidator.rs"],
    );
  });

  it("emits validate_datasource_* with nonnegative and length checks", async () => {
    const user = await userBody();
    assert.match(
      user,
      /pub fn validate_datasource_user\(obj: &crate::types::generated::datasource::User\)/,
    );
    assert.match(user, /must be nonnegative/);
    assert.match(user, /must be at least 3 chars/);
    assert.match(user, /exceeds 256 chars/);
    assert.match(user, /must be at least 0/);
    assert.doesNotMatch(user, /nick_name/);
  });

  it("writes codegen.schema_version into the file header", async () => {
    const user = await userBody({ "codegen.schema_version": "9.9" });
    assert.match(user, /schema-version: 9.9/);
  });
});
