import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import {
  DATASOURCE_TYPES_YAML,
  VIEW_TYPES_YAML,
} from "../src/specification-parser.ts";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "../src/generate-view-types-tests.ts";

const DS_YAML = `types:
  - user:
      datasource_type: audit
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
      datasource_type: readonly-lookup
      fields:
        - name:
            type: string
            is_unique: true
  - tag:
      fields:
        - label:
            type: string
  - wide:
      fields:
        - u32_val:
            type: unsignedinteger
        - u16_val:
            type: unsignedsmallinteger
        - u64_val:
            type: unsignedbiginteger
        - i32_val:
            type: integer
        - i16_val:
            type: smallinteger
`;

const VIEW_YAML = `includes:
  - datasource_types:
      include: "*"
      auto_enrich: true
types:
  - user_summary:
      inherits: datasource_types.user
      omit:
        - nick_name
      fields:
        - display_name:
            type: string
  - payment:
      one_of:
        - card_payment
        - cash_payment
  - card_payment:
      fields:
        - amount:
            type: decimal
        - paid_at:
            type: datetime
        - count:
            type: number
        - rank:
            type: integer
        - small_rank:
            type: smallinteger
        - big_rank:
            type: biginteger
        - score:
            type: float
        - active:
            type: boolean
        - token:
            type: uuid
        - avatar:
            type: binary
        - initial:
            type: character
        - ref_id:
            type: reference
        - tags:
            type: datasource_types.tag[]
        - ghost:
            type: datasource_types.missing
        - owner:
            type: user_summary
        - note:
            type: string
            is_nullable: true
        - flags:
            type: boolean[]
            is_nullable: true
  - cash_payment:
      fields:
        - amount:
            type: decimal
  - tagged:
      inherits: datasource_types.tag
      fields:
        - extra:
            type: string
  - empty_view:
      fields: []
  - empty_union:
      one_of: []
  - wraps_empty:
      fields:
        - inner:
            type: empty_union
  - orphan:
      inherits: datasource_types.missing
      fields:
        - label:
            type: string
`;

const SIMPLE_VIEW_YAML = `types:
  - card_payment:
      fields:
        - amount:
            type: decimal
        - paid_at:
            type: datetime
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

describe("generate view types tests", () => {
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

  it("rejects a datasource_types include without datasource_types.yaml", async () => {
    await assert.rejects(
      () =>
        generate({
          reader: memoryReader({
            [VIEW_TYPES_YAML]: `includes:
  - datasource_types:
      include: "*"
types: []
`,
          }),
          settings: {},
        }),
      /no datasource_types\.yaml was provided/,
    );
  });

  it("emits one test file per expanded view", async () => {
    const byName = indexEntries(await generateWith({}));
    assert.deepEqual(
      [...byName.keys()].sort(),
      [
        "cardPaymentTests.rs",
        "cashPaymentTests.rs",
        "emptyUnionTests.rs",
        "emptyViewTests.rs",
        "orphanTests.rs",
        "paymentTests.rs",
        "roleTests.rs",
        "tagTests.rs",
        "taggedTests.rs",
        "updateTagTests.rs",
        "updateTaggedTests.rs",
        "updateUserSummaryTests.rs",
        "updateUserTests.rs",
        "updateWideTests.rs",
        "userSummaryTests.rs",
        "userTests.rs",
        "wideTests.rs",
        "wrapsEmptyTests.rs",
      ],
    );
  });

  it("renders primitive, array, nested, and nullable accessor cases", async () => {
    const card = await bodyOf("cardPaymentTests.rs");
    assert.match(card, /schema-version: 1\.0/);
    assert.match(card, /use super::cardPayment::\*;/);
    assert.match(card, /fn sample\(\) -> CardPayment \{/);
    assert.match(card, /amount: String::from\("0"\)/);
    assert.match(
      card,
      /paid_at: chrono::DateTime::parse_from_rfc3339\("2024-01-01T00:00:00.000Z"\)/,
    );
    assert.match(card, /count: 1i64/);
    assert.match(card, /rank: 1i32/);
    assert.match(card, /small_rank: 1i16/);
    assert.match(card, /score: 1\.0f64/);
    assert.match(card, /active: false/);
    assert.match(card, /token: String::from\("00000000-0000-0000-0000-000000000000"\)/);
    assert.match(card, /avatar: Vec::<u8>::new\(\)/);
    assert.match(card, /tags: vec!\[crate::types::generated::datasource::Tag \{/);
    assert.match(card, /ghost: crate::types::generated::datasource::Missing \{\}/);
    assert.match(card, /owner: crate::types::generated::views::UserSummary \{/);
    assert.match(card, /note: Some\(String::from\("sample"\)\)/);
    assert.match(card, /flags: Some\(vec!\[false\]\)/);
    assert.match(card, /fn gets_note\(/);
    assert.match(card, /fn allows_setting_note_to_none\(/);
    assert.doesNotMatch(card, /fn allows_setting_amount_to_none\(/);
  });

  it("renders a union view with member constructors", async () => {
    const payment = await bodyOf("paymentTests.rs");
    assert.match(payment, /fn accepts_card_payment_member\(/);
    assert.match(payment, /fn accepts_cash_payment_member\(/);
    assert.match(payment, /Payment::CardPayment\(/);
    assert.match(payment, /let _ok: Payment = value;/);
  });

  it("inlines inherited fields, aliases pass-throughs, and wraps a plain inherit", async () => {
    const summary = await bodyOf("userSummaryTests.rs");
    assert.match(summary, /fn gets_display_name\(/);
    assert.match(summary, /fn gets_email\(/);
    assert.doesNotMatch(summary, /fn gets_nick_name\(/);
    assert.doesNotMatch(summary, /fn gets_role_id\(/);
    const tag = await bodyOf("tagTests.rs");
    assert.match(tag, /fn gets_label\(/);
    assert.match(tag, /fn gets_id\(/);
    const tagged = await bodyOf("taggedTests.rs");
    assert.match(tagged, /base: crate::types::generated::datasource::Tag \{/);
    assert.match(tagged, /fn gets_extra\(/);
    const orphan = await bodyOf("orphanTests.rs");
    assert.match(orphan, /fn gets_label\(/);
    assert.doesNotMatch(orphan, /fn gets_id\(/);
    const empty = await bodyOf("emptyViewTests.rs");
    assert.match(empty, /fn sample\(\) -> EmptyView \{\n        EmptyView \{\}\n    \}/);
    const union = await bodyOf("emptyUnionTests.rs");
    assert.doesNotMatch(union, /fn accepts_/);
    const wrap = await bodyOf("wrapsEmptyTests.rs");
    assert.match(wrap, /inner: crate::types::generated::views::EmptyUnion \{\}/);
  });

  it("covers unsigned inherited columns and nullable parent fields", async () => {
    const wide = await bodyOf("wideTests.rs");
    assert.match(wide, /u32_val: 1u32/);
    assert.match(wide, /u16_val: 1u16/);
    assert.match(wide, /u64_val: 1u64/);
    assert.match(wide, /i32_val: 1i32/);
    assert.match(wide, /i16_val: 1i16/);
    const user = await bodyOf("userTests.rs");
    assert.match(user, /nick_name: Some\(String::from\("sample"\)\)/);
    assert.match(user, /fn allows_setting_nick_name_to_none\(/);
  });

  it("writes codegen.schema_version into the file header", async () => {
    const card = await bodyOf("cardPaymentTests.rs", {
      "codegen.schema_version": "9.9",
    });
    assert.match(card, /schema-version: 9.9/);
  });

  it("uses uuid ids when datasource.id_type=uuid", async () => {
    const user = await bodyOf("userTests.rs", { "datasource.id_type": "uuid" });
    assert.match(user, /id: String::from\("00000000-0000-0000-0000-000000000000"\)/);
    assert.doesNotMatch(user, /fn gets_uuid\(/);
  });

  it("uses i64 ids when datasource.id_type=biginteger", async () => {
    const user = await bodyOf("userTests.rs", {
      "datasource.id_type": "biginteger",
    });
    assert.match(user, /id: 1i64,/);
  });

  it("rejects a cyclic view reference", async () => {
    await assert.rejects(
      () =>
        generateWith(
          {},
          `types:
  - looped:
      fields:
        - other:
            type: looped
`,
          undefined,
        ),
      /cyclic view reference: looped/,
    );
  });

  it("rejects an unknown nested view", async () => {
    await assert.rejects(
      () =>
        generateWith(
          {},
          `types:
  - broken:
      fields:
        - other:
            type: missing_view
`,
          undefined,
        ),
      /unknown view: missing_view/,
    );
  });

  it("rejects a cyclic union member", async () => {
    await assert.rejects(
      () =>
        generateWith(
          {},
          `types:
  - looped:
      one_of:
        - looped
`,
          undefined,
        ),
      /cyclic view reference: looped/,
    );
  });
});
