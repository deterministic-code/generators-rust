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
  - tagged:
      tags: [view_type]
      inherits: tag
      fields:
        - extra:
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
    const tagged = await bodyOf("tagged.rs");
    assert.match(tagged, /pub struct Tagged/);
    assert.match(tagged, /pub id:/);
    assert.match(tagged, /pub label:/);
    assert.match(tagged, /pub extra:/);
    assert.doesNotMatch(tagged, /pub base:/);
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
        - local_now:
            type: datetime
            default_value: Now
        - explicit_at:
            type: datetime
            default_value: "DateTime('2026-01-01T00:00:00Z')"
        - empty_id:
            type: uuid
            default_value: Empty
        - fixed_id:
            type: uuid
            default_value: "uuid('11111111-1111-4111-8111-111111111111')"
        - empty_blob:
            type: binary
            default_value: "Hex('')"
        - initial:
            type: character
            default_value: X
        - ratio:
            type: float
            default_value: 1.5
        - visits:
            type: unsignedinteger
            default_value: 1
        - delta:
            type: smallinteger
            default_value: 1
        - port:
            type: unsignedsmallinteger
            default_value: 1
        - big_rank:
            type: biginteger
            default_value: 1
        - huge:
            type: unsignedbiginteger
            default_value: 1
        - amount:
            type: number
            default_value: 1
        - tags:
            type: string[]
            default_value: []
        - extra:
            type: string[]
        - attrs:
            type: labels{}
            references: labels.key
            default_value: {}
        - aliases:
            type: string[]
            is_nullable: true
            default_value: null
        - home:
            type: place
  - labels:
      tags: [view_type]
      inherits: dictionary
      fields:
        - key:
            type: string
            size: 64
        - value:
            type: string
            size: 128
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
    assert.match(flags, /local_now: chrono::Utc::now\(\),/);
    assert.match(flags, /empty_id: uuid::Uuid::nil\(\)\.to_string\(\),/);
    assert.match(
      flags,
      /fixed_id: String::from\("11111111-1111-4111-8111-111111111111"\),/,
    );
    assert.match(flags, /initial: String::from\("X"\),/);
    assert.match(flags, /ratio: 1.5,/);
    assert.match(flags, /visits: 1,/);
    assert.match(flags, /tags: vec!\[\],/);
    assert.match(flags, /extra: vec!\[\],/);
    assert.match(flags, /attrs: HashMap::new\(\),/);
    assert.match(flags, /aliases: None,/);
    assert.match(flags, /home: .*Place::default\(\)/);
    assert.match(place, /impl Default for Place/);
    assert.match(place, /city: String::new\(\),/);
  });
});

describe("generate datasources rust views", () => {
  it("skips identity aliases and uses local type names", async () => {
    const entries = await generate({
      reader: memoryReader({
        [TYPES_YAML]: `types:
  - user:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - email:
            type: string
  - user_summary:
      tags: [view_type]
      inherits: user
      fields:
        - display_name:
            type: string
`,
      }),
      settings: { "backend.datasources": "sqlite" },
    });
    const names = entries.map((entry) => entry.filename).sort();
    assert.equal(names.some((name) => name.endsWith("user.rs")), false);
    const summary = entries.find((entry) => entry.filename.endsWith("userSummary.rs"));
    assert.ok(summary);
    const body = "contents" in summary! ? String(summary.contents) : summary.content;
    assert.match(body, /#\[derive\(Clone, Debug, PartialEq, Default, datasources::Facet\)\]/);
    assert.doesNotMatch(body, /crate::types::generated/);
  });
});
