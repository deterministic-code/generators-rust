import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "./common/deterministic-reader.ts";
import { DATASOURCE_TYPES_YAML } from "./common/parse-datasource-types.ts";
import { VIEW_TYPES_YAML } from "./common/parse-view-types.ts";
import type { GenerateEntry } from "./common/generate-entry.ts";
import { generate } from "./generate-view-type-validators-tests.ts";

const DS_YAML = `types:
  - user:
      datasource_type: audit
      fields:
        - email:
            type: string
        - nick_name:
            type: string
            is_nullable: true
  - tag:
      fields:
        - label:
            type: string
`;

const VIEW_YAML = `includes:
  - datasource_types:
      include: "*"
types:
  - payment:
      one_of:
        - card_payment
        - cash_payment
  - card_payment:
      fields:
        - amount:
            type: decimal
        - note:
            type: string
            is_nullable: true
        - tags:
            type: datasource_types.tag[]
  - cash_payment:
      fields:
        - amount:
            type: decimal
`;

const SIMPLE_VIEW_YAML = `types:
  - card_payment:
      fields:
        - amount:
            type: decimal
        - note:
            type: string
            is_nullable: true
`;

const fixtureReader = (
  viewYaml: string = VIEW_YAML,
  dsYaml: string | undefined = DS_YAML,
) =>
  memoryReader({
    [VIEW_TYPES_YAML]: viewYaml,
    ...(dsYaml === undefined ? {} : { [DATASOURCE_TYPES_YAML]: dsYaml }),
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

describe("generate view type validators tests", () => {
  const generateWith = (
    settings: Record<string, string> = {},
    viewYaml?: string,
    dsYaml?: string,
  ) =>
    generate({
      reader: fixtureReader(viewYaml, dsYaml),
      settings,
    });

  const bodyOf = async (
    suffix: string,
    settings: Record<string, string> = {},
    viewYaml?: string,
    dsYaml?: string,
  ) => {
    const map = indexEntries(await generateWith(settings, viewYaml, dsYaml));
    const file = [...map.keys()].find((name) => name.endsWith(suffix));
    assert.ok(file, `missing ${suffix} generate entry`);
    return entryBody(requireEntry(map, file));
  };

  it("rejects a missing view_types.yaml", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({}),
          settings: {},
        }),
      /missing view_types\.yaml/,
    );
  });

  it("covers parse and nullable cases for a shaped view", async () => {
    const card = await bodyOf(
      "card_payment_tests.rs",
      {},
      SIMPLE_VIEW_YAML,
      undefined,
    );
    assert.match(card, /fn parses_a_valid_payload/);
    assert.match(card, /fn accepts_none_for_nullable_fields/);
    assert.match(card, /validate_card_payment\(&value\)\.is_ok\(\)/);
    assert.match(card, /note: None/);
  });

  it("emits union member accept cases", async () => {
    const payment = await bodyOf("payment_tests.rs");
    assert.match(payment, /fn accepts_card_payment_member/);
    assert.match(payment, /fn accepts_cash_payment_member/);
    assert.match(payment, /validate_payment\(&value\)\.is_ok\(\)/);
  });

  it("writes codegen.schema_version into the file header", async () => {
    const card = await bodyOf(
      "card_payment_tests.rs",
      { "codegen.schema_version": "9.9" },
      SIMPLE_VIEW_YAML,
      undefined,
    );
    assert.match(card, /schema-version: 9.9/);
  });
});
