import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "./common/deterministic-reader.ts";
import { DATASOURCE_TYPES_YAML } from "./common/parse-datasource-types.ts";
import type { GenerateEntry } from "./common/generate-entry.ts";
import { generate } from "./generate-datasource-type-validators.ts";

const FIXTURE_YAML = `types:
  - user:
      datasource_type: audit
      fields:
        - email:
            type: string
            size: 256
            min_size: 3
        - role_id:
            references: role.id
        - nick_name:
            type: string
            is_nullable: true
        - score:
            type: float
            min_size: 0
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

describe("generate datasource type validators", () => {
  const generateWith = (settings: Record<string, string> = {}) =>
    generate({
      reader: fixtureReader(),
      settings,
    });

  const userBody = async (settings: Record<string, string> = {}) => {
    const map = indexEntries(await generateWith(settings));
    const userFile = [...map.keys()].find((name) =>
      name.endsWith("user_validator.rs"),
    );
    assert.ok(userFile, "missing user_validator.rs generate entry");
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

  it("emits one validator file per datasource type", async () => {
    const byName = indexEntries(await generateWith({}));
    assert.deepEqual(
      [...byName.keys()].sort(),
      ["datasource_role_validator.rs", "datasource_user_validator.rs"],
    );
  });

  it("nests validators under features when organize_by_feature is set", async () => {
    const nested = await generateWith({
      "other.organize_by_feature": "true",
    });
    assert.deepEqual(
      nested.map((e) => e.filename).sort(),
      [
        "features/role/role_validator.rs",
        "features/user/user_validator.rs",
      ],
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

  it("uses the by-feature type path when organize_by_feature is set", async () => {
    const user = await userBody({ "other.organize_by_feature": "true" });
    assert.match(
      user,
      /pub fn validate_datasource_user\(obj: &crate::features::user::user::User\)/,
    );
  });

  it("drops uuid checks when datasource.id_type=uuid", async () => {
    const user = await userBody({ "datasource.id_type": "uuid" });
    assert.match(user, /obj\.id\.to_string\(\)/);
    assert.doesNotMatch(user, /obj\.uuid/);
  });

  it("writes codegen.schema_version into the file header", async () => {
    const user = await userBody({ "codegen.schema_version": "9.9" });
    assert.match(user, /schema-version: 9.9/);
  });
});
