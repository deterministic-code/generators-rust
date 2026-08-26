import pluralize from "pluralize";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  identityColumns,
  isReadonlyLookup,
  tableByName,
} from "@deterministic-code/generators-common/spec-types";
import { convertSpecType } from "./base-type-converter.ts";
import {
  DeterministicParser,
  ROUTES_YAML,
  type DatasourceTable,
  type NestedRouteDescriptor,
  type RouteByField,
  type RouteCandidate,
  type IDeterministic,
  type Type,
} from "./specification-parser.ts";
import {
  appWiringBodyTmpl,
  appWiringTmpl,
  byFieldMergeTmpl,
  checkEagerChildTmpl,
  checkOptionalTypedTmpl,
  checkRequiredTmpl,
  checkRequiredTypedTmpl,
  coerceRowTmpl,
  crudTmpl,
  readonlyTmpl,
  validatorEmptyTmpl,
  validatorTmpl,
} from "./resources/routes.ts";
import { Emit } from "./emit.ts";

type Field = {
  name: string;
  type: string;
  isNullable?: boolean;
  hasDefault?: boolean;
};

type EagerChild = {
  fieldName: string;
  fkColumn: string;
  childTable: string;
  kind?: string;
};

type PrimaryKey = {
  column: string;
  idType?: string;
  rustType?: string;
};

type NormalizedByField = {
  byField: string;
  unique: boolean;
};

const pluralSnakeField = (entity: string): string => {
  const parts = entity.split(/[_-]/);
  parts[parts.length - 1] = pluralize.plural(parts[parts.length - 1]!);
  return parts.join("_");
};

const normalizeByFields = (byFields: RouteByField[]): NormalizedByField[] =>
  byFields
    .map((e): NormalizedByField | null => {
      if (!e.byField) return null;
      const methods = Array.isArray(e.methods)
        ? e.methods.filter((m) => m === "GET")
        : ["GET"];
      if (methods.length === 0) return null;
      return { byField: e.byField, unique: e.byFieldUnique === true };
    })
    .filter((e): e is NormalizedByField => e !== null);

const rustStrSlice = (names: string[]): string =>
  names.length > 0
    ? `&[${names.map((n) => JSON.stringify(n)).join(", ")}]`
    : "&[]";

const byFieldPrimitiveCall = (
  entitySnake: string,
  apiPath: string,
  entry: NormalizedByField,
  hasCoercion: boolean,
): string =>
  fill(byFieldMergeTmpl, {
    entitySnake,
    path: `/api/${apiPath}`,
    byField: entry.byField,
    unique: entry.unique ? "true" : "false",
    coerceExpr: hasCoercion
      ? "Some(Arc::new(|row: &mut RowMap| coerce_row_types(row)))"
      : "None",
  }).trimEnd();

const byFieldMergeChain = (
  entitySnake: string,
  apiPath: string,
  normalizedByFields: NormalizedByField[],
  hasCoercion: boolean,
): string =>
  normalizedByFields
    .map((e) => byFieldPrimitiveCall(entitySnake, apiPath, e, hasCoercion))
    .join("\n");

const fieldTypeCheck = (type: string): string | null => {
  switch (type) {
    case "string":
    case "uuid":
      return "is_string";
    case "number":
    case "biginteger":
    case "reference":
      return "is_number";
    case "boolean":
      return "is_boolean";
    case "datetime":
      return "is_string";
    default:
      return null;
  }
};

const buildEagerChildShapeCheck = (child: EagerChild): string =>
  fill(checkEagerChildTmpl, {
    name: child.fieldName,
    nameJson: JSON.stringify(child.fieldName),
  }).trimEnd();

const requiredFieldCheck = (f: Field, requireAll: boolean): string => {
  const typeCheck = fieldTypeCheck(f.type);
  if (requireAll) {
    if (typeCheck) {
      return fill(checkRequiredTypedTmpl, {
        name: f.name,
        typeCheck,
        type: f.type,
      }).trimEnd();
    }
    return fill(checkRequiredTmpl, { name: f.name }).trimEnd();
  }
  if (typeCheck) {
    return fill(checkOptionalTypedTmpl, {
      name: f.name,
      typeCheck,
      type: f.type,
    }).trimEnd();
  }
  return "";
};

const buildValidatorFn = (args: {
  fnName: string;
  requiredFields: Field[];
  requireAll: boolean;
  directFkChildren: EagerChild[];
}): string => {
  const checks = [
    ...args.requiredFields.map((f) => requiredFieldCheck(f, args.requireAll)),
    ...args.directFkChildren.map(buildEagerChildShapeCheck),
  ]
    .filter((s) => s.length > 0)
    .join("\n");
  if (checks.length === 0) {
    return fill(validatorEmptyTmpl, { fnName: args.fnName }).trimEnd();
  }
  return fill(validatorTmpl, { fnName: args.fnName, checks }).trimEnd();
};

const generateCoerceRowFn = (
  booleanFields: Field[],
  binaryFields: Field[],
): string =>
  fill(coerceRowTmpl, {
    boolCols: rustStrSlice(booleanFields.map((f) => f.name)),
    binCols: rustStrSlice(binaryFields.map((f) => f.name)),
  }).trimEnd();

const partitionFieldsByType = (
  fields: Field[],
): { booleanFields: Field[]; binaryFields: Field[] } => ({
  booleanFields: fields.filter((f) => f.type === "boolean"),
  binaryFields: fields.filter((f) => f.type === "binary"),
});

const idTypeVariantForPk = (primaryKey: PrimaryKey): string => {
  switch (primaryKey.idType) {
    case "uuid":
      return "Uuid";
    case "string":
      return "String";
    case "integer":
    case "biginteger":
      return "Integer";
    default:
      return primaryKey.rustType === "String" ? "String" : "Integer";
  }
};

const inferPrimaryKey = (
  type: Type | undefined,
  table: DatasourceTable | undefined,
): PrimaryKey => inferPrimaryKeys(type, table)[0]!;

const inferPrimaryKeys = (
  type: Type | undefined,
  table: DatasourceTable | undefined,
): PrimaryKey[] => {
  const columns =
    type !== undefined ? identityColumns(type, table) : ["id"];
  const names = columns.length > 0 ? columns : ["id"];
  return names.map((column) => {
    const field = type?.fields.find((f) => f.name === column);
    const pkType = field?.type ?? "integer";
    if (column !== "id" && field !== undefined) {
      return {
        column,
        rustType: field.type === "number" ? "i64" : "String",
        idType: pkType,
      };
    }
    return {
      column,
      rustType: convertSpecType(field?.type ?? "integer"),
      idType: pkType,
    };
  });
};

const sortedDirectFkChildren = (
  eagerWriteChildren: EagerChild[],
): EagerChild[] =>
  eagerWriteChildren
    .filter((c) => (c.kind ?? "direct-fk") === "direct-fk")
    .slice()
    .sort((a, b) => a.fieldName.localeCompare(b.fieldName));

const eagerWriteChildrenForEntity = (
  entity: string,
  nested: NestedRouteDescriptor[],
): EagerChild[] =>
  nested
    .filter(
      (d): d is Extract<NestedRouteDescriptor, { kind: "direct-fk" }> =>
        d.kind === "direct-fk" && d.parent === entity,
    )
    .map((d) => ({
      fieldName: pluralSnakeField(d.child.name),
      fkColumn: d.fkColumn,
      childTable: d.child.name,
      kind: "direct-fk",
    }));

const fieldsForEntity = (entity: string, types: Type[]): Field[] => {
  const ds = types.find((d) => d.name === entity);
  if (ds === undefined) return [];
  return ds.fields.map((f) => ({
    name: f.name,
    type: f.type,
    isNullable: f.isNullable,
    hasDefault: f.hasDefault,
  }));
};

const buildCrudValidators = (
  createFnName: string,
  updateFnName: string,
  effectiveRequiredFields: Field[],
  children: EagerChild[],
): { createValidator: string; updateValidator: string } => ({
  createValidator: buildValidatorFn({
    fnName: createFnName,
    requiredFields: effectiveRequiredFields,
    requireAll: true,
    directFkChildren: children,
  }),
  updateValidator: buildValidatorFn({
    fnName: updateFnName,
    requiredFields: effectiveRequiredFields,
    requireAll: false,
    directFkChildren: children,
  }),
});

class Generator extends Emit {
  private readonly types: Type[];
  private readonly tables: Map<string, DatasourceTable>;
  private readonly nested: NestedRouteDescriptor[];

  constructor(
    raw: Record<string, string>,
    types: Type[],
    tables: Map<string, DatasourceTable>,
    nested: NestedRouteDescriptor[],
  ) {
    super(raw);
    this.types = types;
    this.tables = tables;
    this.nested = nested;
  }

  private typeOf(name: string): Type | undefined {
    return this.types.find((t) => t.name === name);
  }

  from(deterministic: IDeterministic): GenerateEntry[] {
    const parsed = deterministic.routes;
    const entries: GenerateEntry[] = parsed.candidates.map((c) =>
      this.entityRouter(c),
    );
    if (parsed.candidates.length > 0) {
      entries.push(this.appWiring(parsed.candidates));
    }
    return entries;
  }

  private entityRouter(candidate: RouteCandidate): GenerateEntry {
    const type = this.typeOf(candidate.name);
    return type !== undefined && isReadonlyLookup(type)
      ? this.readOnly(candidate)
      : this.crud(candidate);
  }

  private readOnly(candidate: RouteCandidate): GenerateEntry {
    const entitySnake = candidate.name;
    const className = this.casing.serviceClassName(entitySnake);
    const path = `/api/${this.imports.apiPath(entitySnake)}`;
    const pk = inferPrimaryKey(
      this.typeOf(entitySnake),
      this.tables.get(entitySnake),
    );
    const normalizedByFields = normalizeByFields(candidate.byFields);
    const tokens = {
      serviceImport: this.imports.serviceUse(entitySnake, className),
      className,
      entitySnake,
      path,
      idTypeVariant: idTypeVariantForPk(pk),
      hasByFields: normalizedByFields.length > 0,
      byFieldMerges: byFieldMergeChain(
        entitySnake,
        this.imports.apiPath(entitySnake),
        normalizedByFields,
        false,
      ),
    };
    return content(this.imports.route(entitySnake), fill(readonlyTmpl, tokens));
  }

  private crud(candidate: RouteCandidate): GenerateEntry {
    const entitySnake = candidate.name;
    const className = this.casing.serviceClassName(entitySnake);
    const path = `/api/${this.imports.apiPath(entitySnake)}`;
    const allFields = fieldsForEntity(entitySnake, this.types);
    const eagerWriteChildren = eagerWriteChildrenForEntity(
      entitySnake,
      this.nested,
    );
    const keys = inferPrimaryKeys(
      this.typeOf(entitySnake),
      this.tables.get(entitySnake),
    );
    const pk = keys[0]!;
    const normalizedByFields = normalizeByFields(candidate.byFields);
    const hasByFields = normalizedByFields.length > 0;
    const directFkChildren = sortedDirectFkChildren(eagerWriteChildren);
    const { booleanFields, binaryFields } = partitionFieldsByType(allFields);
    const hasCoercion = booleanFields.length + binaryFields.length > 0;
    const coerceFn = hasCoercion
      ? `\n\n${generateCoerceRowFn(booleanFields, binaryFields)}`
      : "";
    const createValidatorName = this.casing.fnIdent(
      `validate_create_${entitySnake}`,
    );
    const updateValidatorName = this.casing.fnIdent(
      `validate_update_${entitySnake}`,
    );
    const { createValidator, updateValidator } = buildCrudValidators(
      createValidatorName,
      updateValidatorName,
      [],
      directFkChildren,
    );
    const type = this.typeOf(candidate.name);
    const table = this.tables.get(candidate.name);
    const occ = this.settings.usesOptimisticConcurrency({
      tags: type?.tags ?? candidate.tags,
      useOptimisticConcurrency: table?.useOptimisticConcurrency,
    });
    const tokens = {
      serviceImport: this.imports.serviceUse(entitySnake, className),
      className,
      entitySnake,
      path,
      idTypeVariant: idTypeVariantForPk(pk),
      primaryKeyParamExpr:
        pk.column === "id" ? "None" : `Some("${pk.column}".to_string())`,
      primaryKeyParamsExpr:
        keys.length > 1
          ? `vec![${keys
              .map((k) => `"${k.column}".to_string()`)
              .join(", ")}]`
          : "vec![]",
      useOptimisticConcurrency: occ ? "true" : "false",
      coerceRowExpr: hasCoercion
        ? "Some(Arc::new(|row: &mut RowMap| coerce_row_types(row)))"
        : "None",
      hasCoercion,
      hasByFields,
      createValidatorName,
      updateValidatorName,
      createValidator,
      updateValidator,
      coerceFn,
      byFieldMerges: byFieldMergeChain(
        entitySnake,
        this.imports.apiPath(entitySnake),
        normalizedByFields,
        hasCoercion,
      ),
    };
    return content(this.imports.route(entitySnake), fill(crudTmpl, tokens));
  }

  private appWiring(candidates: RouteCandidate[]): GenerateEntry {
    const services = candidates.map((c) => ({
      fieldName: this.casing.convertFields(`${c.name}_service`),
      className: this.casing.serviceClassName(c.name),
      routeModule: this.imports.spec("", this.imports.routeRel(c.name)),
    }));
    return content(
      this.imports.appWiring(),
      fill(appWiringTmpl, {
        imports: candidates.map((c) =>
          this.imports.serviceUse(c.name, this.casing.serviceClassName(c.name)),
        ),
        body: fill(appWiringBodyTmpl, {
          hasServices: services.length > 0,
          services,
        }).trimEnd(),
      }),
    );
  }
}

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  await ctx.reader.read(ROUTES_YAML);
  const deterministic = await DeterministicParser(ctx.reader).parse(
    ctx.settings,
  );
  return new Generator(
    ctx.settings,
    deterministic.expandedTypes,
    tableByName(deterministic),
    deterministic.routes.nested,
  ).from(deterministic);
};
