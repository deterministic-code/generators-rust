import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "./common/deterministic-reader.ts";
import { DATASOURCE_TYPES_YAML } from "./common/parse-datasource-types.ts";
import { VIEW_TYPES_YAML } from "./common/parse-view-types.ts";
import type { GenerateEntry } from "./common/generate-entry.ts";
import { generate } from "./generate-view-type-validators.ts";

const DS_YAML = `types:
  - user:
      datasource_type: audit
      fields:
        - email:
            type: string
        - role_id:
            type: number
            references: role.id
  - tag:
      fields:
        - label:
            type: string
`;

const VIEW_YAML = `includes:
  - datasource_types:
      include: "*"
      auto_enrich: true
types:
  - payment:
      one_of:
        - card_payment
        - cash_payment
  - card_payment:
      fields:
        - amount:
            type: decimal
        - tags:
            type: datasource_types.tag[]
        - owner:
            type: user
            is_nullable: true
  - cash_payment:
      fields:
        - amount:
            type: decimal
`;

const fixtureReader = () =>
  memoryReader({
    [VIEW_TYPES_YAML]: VIEW_YAML,
    [DATASOURCE_TYPES_YAML]: DS_YAML,
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

describe("generate view type validators", () => {
  const bodyOf = async (suffix: string) => {
    const map = indexEntries(
      await generate({ reader: fixtureReader(), settings: {} }),
    );
    const file = [...map.keys()].find((name) => name.endsWith(suffix));
    assert.ok(file, `missing ${suffix} generate entry`);
    return entryBody(requireEntry(map, file));
  };

  it("rejects a missing view_types.yaml", async () => {
    await assert.rejects(
      () => generate({ reader: memoryReader({}), settings: {} }),
      /missing view_types\.yaml/,
    );
  });

  it("validates nested fields and union members", async () => {
    const card = await bodyOf("card_payment_validator.rs");
    assert.match(card, /pub fn validate_card_payment/);
    assert.match(card, /for item in &obj.tags/);
    assert.match(card, /if let Some\(inner\) = &obj.owner/);
    const payment = await bodyOf("payment_validator.rs");
    assert.match(payment, /pub fn validate_payment/);
    assert.match(payment, /Payment::CardPayment\(inner\) =>/);
  });
});
