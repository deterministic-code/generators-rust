import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { parseDatasourceTypes } from "./parse-datasource-types.ts";

describe("parseDatasourceTypes", () => {
  it("reads types: and inherits a type-less references: parent.id", () => {
    const types = parseDatasourceTypes({
      idType: "integer",
      yaml: `types:
  - user:
      fields:
        - role_id:
            references: role.id
  - role:
      fields:
        - name:
            type: string
`,
    });
    assert.deepEqual(
      types.map((t) => t.name),
      ["user", "role"],
    );
    assert.equal(types[0]?.datasourceType, "standard");
    assert.deepEqual(types[0]?.fields, [
      {
        name: "role_id",
        type: "number",
        isNullable: false,
        references: "role.id",
      },
    ]);
  });

  it("uses an explicit primary_key type when the reference targets that column", () => {
    const types = parseDatasourceTypes({
      idType: "integer",
      yaml: `types:
  - child:
      fields:
        - parent_code:
            references: parent.code
  - parent:
      fields:
        - code:
            type: string
            primary_key: true
`,
    });
    assert.equal(types[0]?.fields[0]?.type, "string");
  });

  it("throws when a type-less reference cannot be resolved", () => {
    assert.throws(
      () =>
        parseDatasourceTypes({
          idType: "integer",
          yaml: `types:
  - user:
      fields:
        - role_id:
            references: missing.id
`,
        }),
      /type-less reference "role_id"/,
    );
  });
});
