import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import { TYPES_YAML } from "../src/specification-parser.ts";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-view-types.ts";

const TYPES = `types:
  - user:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - email:
            type: string
        - role_id:
            type: number
            references: role.id
        - nick_name:
            type: string
            is_nullable: true
  - role:
      tags: [datasource_type, view_type, readonly_lookup]
      inherits: set
      fields:
        - name:
            type: string
  - tag:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - label:
            type: string
  - user_summary:
      tags: [view_type]
      inherits: user
      remove_fields: [nick_name, role_id]
      fields:
        - display_name:
            type: string
  - payment:
      tags: [view_type]
      union:
        - card_payment
        - cash_payment
  - card_payment:
      tags: [view_type]
      fields:
        - amount:
            type: decimal
        - tags:
            type: tag[]
        - note:
            type: string
            is_nullable: true
  - cash_payment:
      tags: [view_type]
      fields:
        - tendered:
            type: decimal
`;

const fixtureReader = () =>
  memoryReader({
    [TYPES_YAML]: TYPES,
  });

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

describe("generate view types", () => {
  const bodyOf = async (suffix: string, settings: Record<string, string> = {}) => {
    const map = indexEntries(
      await generate({ reader: fixtureReader(), settings }),
    );
    const file = [...map.keys()].find((name) => name.endsWith(suffix));
    assert.ok(file, `missing ${suffix} generate entry`);
    return entryBody(requireEntry(map, file));
  };

  it("rejects a missing types.yaml", async () => {
    await assert.rejects(
      () => generate({ reader: memoryReader({}), settings: {} }),
      /missing types\.yaml/,
    );
  });

  it("renders a shaped view, a union enum, and an inlined inherit", async () => {
    const card = await bodyOf("cardPayment.rs");
    assert.match(card, /pub struct CardPayment/);
    assert.match(card, /pub amount: String/);
    assert.match(card, /pub tags: Vec</);
    assert.match(card, /pub note: Option<String>/);
    const payment = await bodyOf("payment.rs");
    assert.match(payment, /pub enum Payment/);
    assert.match(payment, /CardPayment\(/);
    const summary = await bodyOf("userSummary.rs");
    assert.match(summary, /pub struct UserSummary/);
    assert.match(summary, /pub display_name: String/);
    assert.match(summary, /pub email: String/);
    assert.doesNotMatch(summary, /pub nick_name/);
    assert.doesNotMatch(summary, /pub role_id/);
  });

  it("emits Default for non-zero spec defaults and nested structs", async () => {
    const map = indexEntries(
      await generate({
        reader: memoryReader({
          [TYPES_YAML]: `types:
  - flags:
      tags: [view_type]
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
        - home:
            type: place
  - place:
      tags: [view_type]
      fields:
        - city:
            type: string
`,
        }),
        settings: {},
      }),
    );
    const flagsFile = [...map.keys()].find((name) => name.endsWith("flags.rs"));
    const placeFile = [...map.keys()].find((name) => name.endsWith("place.rs"));
    assert.ok(flagsFile, "missing flags.rs generate entry");
    assert.ok(placeFile, "missing place.rs generate entry");
    const flags = entryBody(requireEntry(map, flagsFile));
    const place = entryBody(requireEntry(map, placeFile));
    assert.match(flags, /impl Default for Flags/);
    assert.match(flags, /is_enabled: true,/);
    assert.match(flags, /empty_name: String::new\(\),/);
    assert.doesNotMatch(flags, /String::from\(""\)/);
    assert.match(flags, /opted_out: Some\(false\),/);
    assert.match(flags, /token: uuid::Uuid::new_v4\(\)\.to_string\(\),/);
    assert.match(flags, /price: String::from\("1.50"\),/);
    assert.match(flags, /home: .*Place::default\(\)/);
    assert.match(place, /impl Default for Place/);
    assert.match(place, /city: String::new\(\),/);
  });
});
