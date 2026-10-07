import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { TYPES_YAML } from "../src/specification-parser.ts";
import { generate as generateDatasourceTypes } from "../src/generate-datasource-types.ts";
import { generate as generateViewTypes } from "../src/generate-view-types.ts";

const TYPES = `types:
  - file:
      tags: [datasource_type, view_type]
      inherits: set
      fields:
        - name:
            type: string
            size: 64
        - settings:
            type: settings{}
            references: settings.key
  - settings:
      tags: [datasource_type]
      inherits: dictionary
      fields:
        - setting_id:
            type: integer
            references: file.id
        - key:
            type: string
            size: 64
        - value:
            type: string
            size: unlimited
  - locale_pref:
      tags: [view_type]
      fields:
        - locale:
            type: string
            size: 16
        - timezone:
            type: string
            size: 64
  - contact_prefs:
      tags: [datasource_type]
      inherits: dictionary
      fields:
        - contact_id:
            type: integer
            references: contacts_ds.id
        - key:
            type: string
            size: 64
        - value:
            type: locale_pref
  - contacts_ds:
      tags: [datasource_type]
      inherits: set
      fields:
        - email:
            type: string
            size: 256
  - contact:
      tags: [view_type]
      inherits: contacts_ds
      fields:
        - prefs:
            type: contact_prefs{}
            references: contact_prefs.key
  - card_labels:
      tags: [view_type]
      inherits: dictionary
      fields:
        - key:
            type: string
            size: 64
        - value:
            type: string
            size: 128
  - contact_card:
      tags: [view_type]
      fields:
        - display_name:
            type: string
            size: 256
        - labels:
            type: card_labels{}
            references: card_labels.key
`;

const ctx = {
  reader: memoryReader({ [TYPES_YAML]: TYPES }),
  settings: {},
};

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

const bodyEnding = (map: Map<string, GenerateEntry>, suffix: string): string => {
  const file = [...map.keys()].find((name) => name.endsWith(suffix));
  if (file === undefined) {
    throw new Error(`missing generate entry ending ${suffix}`);
  }
  return entryBody(map.get(file)!);
};

const hasEnding = (map: Map<string, GenerateEntry>, suffix: string): boolean =>
  [...map.keys()].some((name) => name.endsWith(suffix));

describe("owned dictionary codegen", () => {
  it("emits Settings as a row-shaped datasource type", async () => {
    const entries = indexEntries(await generateDatasourceTypes(ctx));
    const settings = bodyEnding(entries, "settings.rs");
    assert.match(settings, /pub struct Settings/);
    assert.match(settings, /pub setting_id:/);
    assert.match(settings, /pub key:/);
    assert.match(settings, /pub value:/);
    assert.doesNotMatch(settings, /pub id:/);
    assert.doesNotMatch(settings, /HashMap</);
    const file = bodyEnding(entries, "file.rs");
    assert.doesNotMatch(file, /settings/);
    assert.doesNotMatch(file, /HashMap</);
    const prefs = bodyEnding(entries, "contactPrefs.rs");
    assert.match(prefs, /pub locale:/);
    assert.match(prefs, /pub timezone:/);
    assert.doesNotMatch(prefs, /pub value:/);
  });

  it("attaches HashMap on views and does not emit dictionary view structs", async () => {
    const entries = indexEntries(await generateViewTypes(ctx));
    assert.equal(hasEnding(entries, "settings.rs"), false);
    assert.equal(hasEnding(entries, "cardLabels.rs"), false);
    assert.equal(hasEnding(entries, "contactPrefs.rs"), false);
    const file = bodyEnding(entries, "file.rs");
    assert.match(file, /use std::collections::HashMap;/);
    assert.match(file, /pub struct File/);
    assert.match(file, /pub settings: HashMap<String, String>/);
    const contact = bodyEnding(entries, "contact.rs");
    assert.match(contact, /pub prefs: HashMap<String, /);
    assert.match(contact, /LocalePref/);
    const card = bodyEnding(entries, "contactCard.rs");
    assert.match(card, /pub labels: HashMap<String, String>/);
    const locale = bodyEnding(entries, "localePref.rs");
    assert.match(locale, /pub struct LocalePref/);
  });
});
