import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  columnFields,
  datasourceTypesOf,
} from "@deterministic-code/generators-common/spec-types";
import type { PackCasing } from "./common/default-casing.ts";
import {
  rustString,
  samplesForNative,
  wrapOption,
} from "./common/test-samples.ts";
import {
  DeterministicParser,
  TYPES_YAML,
  type IDeterministic,
  type Type,
  type TypeField,
} from "./specification-parser.ts";
import { convertSpecType } from "./base-type-converter.ts";
import { typeTestTmpl } from "./resources/datasource-type-validators-tests.ts";
import { Emit } from "./emit.ts";

type FieldTok = {
  name: string;
  ident: string;
  sampleExpr: string;
  isNullable: boolean;
  type: string;
  native: string;
};

type CaseTok = {
  ident: string;
  fixture: string;
  assertion: string;
};

const fieldTok = (
  field: TypeField | { name: string; type: string; isNullable: boolean },
  casing: PackCasing,
): FieldTok => {
  const native = convertSpecType(field.type);
  const { sample } = samplesForNative(native, field.type);
  return {
    name: field.name,
    ident: casing.convertFields(field.name),
    sampleExpr: wrapOption(sample, field.isNullable),
    isNullable: field.isNullable,
    type: field.type,
    native,
  };
};

const structLiteral = (
  cls: string,
  fields: Array<{ ident: string; expr: string }>,
): string =>
  `${cls} { ${fields.map((f) => `${f.ident}: ${f.expr}`).join(", ")} }`;

const casesFor = (cls: string, fields: FieldTok[]): CaseTok[] => {
  const valid = structLiteral(
    cls,
    fields.map((f) => ({ ident: f.ident, expr: f.sampleExpr })),
  );
  const cases: CaseTok[] = [
    { ident: "parses_a_valid_payload", fixture: valid, assertion: "is_ok()" },
  ];
  if (fields.some((f) => f.isNullable)) {
    cases.push({
      ident: "accepts_none_for_nullable_fields",
      fixture: structLiteral(
        cls,
        fields.map((f) => ({
          ident: f.ident,
          expr: f.isNullable ? "None" : f.sampleExpr,
        })),
      ),
      assertion: "is_ok()",
    });
  }
  for (const field of fields) {
    if (field.type === "uuid" && field.native === "String") {
      cases.push({
        ident: `rejects_when_invalid_uuid_on_${field.ident}`,
        fixture: structLiteral(
          cls,
          fields.map((f) => ({
            ident: f.ident,
            expr:
              f.ident === field.ident
                ? wrapOption(rustString("not-a-uuid"), f.isNullable)
                : f.sampleExpr,
          })),
        ),
        assertion: "is_err()",
      });
    }
  }
  return cases;
};

class Generator extends Emit {
  from(deterministic: IDeterministic): GenerateEntry[] {
    return datasourceTypesOf(deterministic).map((table) => this.tests(table));
  }

  private tests(table: Type): GenerateEntry {
    const fields = columnFields(table.fields).map((f) => fieldTok(f, this.casing));
    const cls = this.casing.convertTypes(table.name);
    const src = this.imports.datasource(table.name);
    return content(
      this.imports.test(src, table.name),
      fill(typeTestTmpl, {
        schemaVersion: this.settings.schemaVersion,
        typeUse: this.imports.datasourceQual(table.name),
        fnName: this.casing.convertFields(`validate_datasource_${table.name}`),
        cases: casesFor(cls, fields),
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(TYPES_YAML);
  return new Generator(ctx.settings).from(
    await DeterministicParser(ctx.reader).parse(ctx.settings),
  );
};
