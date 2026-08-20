import assert from "node:assert/strict";
import { before, describe, it } from "node:test";
import { memoryReader } from "@deterministic-code/generators-common/deterministic-reader";
import type { GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import { generate } from "./generate-backend-app.ts";

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
  assert.ok(entry, `missing generate entry: ${filename}`);
  return entry;
};

describe("generate", () => {
  let byName = new Map<string, GenerateEntry>();

  before(async () => {
    byName = indexEntries(
      await generate({
        reader: memoryReader({}),
        settings: { application_name: "catalog-api" },
      }),
    );
  });

  it("emits the flat single-language scaffold", () => {
    assert.deepEqual(
      [...byName.keys()].sort(),
      [
        ".dockerignore",
        ".env",
        ".env.example",
        ".gitignore",
        "Cargo.toml",
        "Dockerfile",
        "docker-compose.yml",
        "scripts/entrypoint.sh",
        "src/lib.rs",
        "src/main.rs",
      ],
    );
    for (const filename of byName.keys()) {
      assert.equal(filename.startsWith("rust/"), false, filename);
      assert.equal(filename.startsWith("backend/"), false, filename);
      assert.equal(filename.startsWith("_deterministic/"), false, filename);
    }
    const dockerignore = requireEntry(byName, ".dockerignore");
    assert.equal(dockerignore.kind, "patch");
    assert.equal(
      "section" in dockerignore ? dockerignore.section : undefined,
      undefined,
    );
    assert.equal(entryBody(dockerignore), "target");
  });

  it("renders main.rs and lib.rs against the crate ident and flat compose_router", () => {
    const main = entryBody(requireEntry(byName, "src/main.rs"));
    assert.equal(requireEntry(byName, "src/main.rs").kind, "content");
    assert.match(main, /catalog_api::custom_services\(\)/);
    assert.match(main, /catalog_api::route_composer\(\)/);
    const lib = entryBody(requireEntry(byName, "src/lib.rs"));
    assert.equal(requireEntry(byName, "src/lib.rs").kind, "patch");
    assert.match(lib, /BEGIN MODULES/);
    assert.match(lib, /BEGIN CUSTOM_SERVICES/);
    assert.match(lib, /crate::routes::generated::app_wiring::compose_router/);
    assert.doesNotMatch(lib, /features::app_wiring/);
  });

  it("renders Cargo.toml for the published crate, not a bundled path dep", () => {
    const cargo = entryBody(requireEntry(byName, "Cargo.toml"));
    assert.match(cargo, /^name = "catalog_api"$/m);
    assert.match(cargo, /deterministic = "0\.0\.6"/);
    assert.doesNotMatch(cargo, /_deterministic\/rust/);
  });

  it("copies the project from the image root, not a language lane", () => {
    const dockerfile = entryBody(requireEntry(byName, "Dockerfile"));
    assert.match(dockerfile, /^COPY Cargo\.toml \.\/$/m);
    assert.match(dockerfile, /^COPY src \.\/src$/m);
    assert.doesNotMatch(dockerfile, /rust\//);
    assert.doesNotMatch(dockerfile, /COPY _deterministic/);
  });

  it("renders a root compose service without a lane dockerfile path", () => {
    const compose = entryBody(requireEntry(byName, "docker-compose.yml"));
    assert.match(compose, /^app:/m);
    assert.match(compose, /HOST_PORT/);
    assert.match(compose, /deterministic\.language=rust/);
    assert.doesNotMatch(compose, /dockerfile:/);
    assert.doesNotMatch(compose, /rust\/Dockerfile/);
  });
});
