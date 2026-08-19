import pluralize from "pluralize";
import { fill } from "@deterministic-code/generators-common/fill";
import type { GenerateContext } from "@deterministic-code/generators-common/generate-context";
import { content, type GenerateEntry } from "@deterministic-code/generators-common/generate-entry";
import {
  routePaths,
  type RoutePaths,
} from "./common/paths.ts";
import { idTypeToNative } from "./common/type-converter.ts";
import {
  SpecificationParser,
  entityUsesOptimisticConcurrency,
  type DatasourceField,
  type DatasourceType,
  type NestedRouteDescriptor,
  type RouteByField,
  type RouteCandidate,
  type ViewEnrichment,
  type ViewType,
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
import { serviceFieldName } from "./rust-eager-service-graph.ts";

type Field = {
  name: string;
  type: string;
  isNullable?: boolean;
  hasDefault?: boolean;
};

type Enrichment = {
  targetTable: string;
  fkColumn: string;
  newField: string;
  prefix?: string;
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

type EmitOptions = {
  naming: RoutePaths;
  idType: string;
  rustIdType: string;
  useOptimisticConcurrency: boolean;
  views: ViewType[];
  datasources: DatasourceType[];
  nested: NestedRouteDescriptor[];
};

const emitOptions = async (
  settings: Record<string, string>,
  views: ViewType[],
  nested: NestedRouteDescriptor[],
  datasources: DatasourceType[],
): Promise<EmitOptions> => {
  const idType = settings["datasource.id_type"] ?? "integer";
  return {
    naming: routePaths(settings),
    idType,
    rustIdType: idTypeToNative(idType),
    useOptimisticConcurrency:
      settings["datasource.use_optimistic_concurrency"] === "true",
    views,
    datasources,
    nested,
  };
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

const applyEnrichmentToRequiredFields = (
  requiredFields: Field[],
  enrichments: Enrichment[],
): Field[] => {
  if (enrichments.length === 0) return requiredFields;
  const requiredNames = new Set(requiredFields.map((f) => f.name));
  const fkSet = new Set(enrichments.map((e) => e.fkColumn));
  const out = requiredFields.filter((f) => !fkSet.has(f.name));
  for (const e of enrichments) {
    if (requiredNames.has(e.fkColumn)) {
      out.push({
        name: e.newField,
        type: "string",
        isNullable: false,
        hasDefault: false,
      });
    }
  }
  return out;
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

const idTypeFromFieldType = (fieldType: string): string => {
  if (fieldType === "string") return "string";
  if (fieldType === "uuid") return "uuid";
  return "integer";
};

const inferPrimaryKey = (
  ds: DatasourceType | undefined,
  opts: EmitOptions,
): PrimaryKey => {
  if (ds !== undefined) {
    const custom = ds.fields.find(
      (f) => f.isPrimaryKey === true && f.name !== "id",
    );
    if (custom !== undefined) {
      return {
        column: custom.name,
        rustType: custom.type === "number" ? "i64" : "String",
        idType: idTypeFromFieldType(custom.type),
      };
    }
  }
  return {
    column: "id",
    rustType: opts.rustIdType,
    idType: opts.idType,
  };
};

const sortedDirectFkChildren = (
  eagerWriteChildren: EagerChild[],
): EagerChild[] =>
  eagerWriteChildren
    .filter((c) => (c.kind ?? "direct-fk") === "direct-fk")
    .slice()
    .sort((a, b) => a.fieldName.localeCompare(b.fieldName));

const enrichmentsForEntity = (
  entity: string,
  views: ViewType[],
): Enrichment[] => {
  const out: Enrichment[] = [];
  for (const view of views) {
    if (view.kind !== "shaped") continue;
    if (view.inherits !== entity && view.name !== entity) continue;
    for (const e of view.enrichments as ViewEnrichment[]) {
      out.push({
        targetTable: e.targetTable,
        fkColumn: e.fkColumn,
        newField: e.newField,
        prefix: e.prefix,
      });
    }
  }
  return out;
};

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

const fieldsForEntity = (
  entity: string,
  datasources: DatasourceType[],
): Field[] => {
  const ds = datasources.find((d) => d.name === entity);
  if (ds === undefined) return [];
  return ds.fields.map((f: DatasourceField) => ({
    name: f.name,
    type: f.type,
    isNullable: f.isNullable,
    hasDefault: f.hasDefault,
  }));
};

const buildCrudValidators = (
  entitySnake: string,
  effectiveRequiredFields: Field[],
  children: EagerChild[],
): { createValidator: string; updateValidator: string } => ({
  createValidator: buildValidatorFn({
    fnName: `validate_create_${entitySnake}`,
    requiredFields: effectiveRequiredFields,
    requireAll: true,
    directFkChildren: children,
  }),
  updateValidator: buildValidatorFn({
    fnName: `validate_update_${entitySnake}`,
    requiredFields: effectiveRequiredFields,
    requireAll: false,
    directFkChildren: children,
  }),
});

const renderReadOnly = (
  candidate: RouteCandidate,
  opts: EmitOptions,
): GenerateEntry => {
  const { naming } = opts;
  const entitySnake = candidate.name;
  const className = naming.serviceClassName(entitySnake);
  const path = `/api/${naming.apiPath(entitySnake)}`;
  const pk = inferPrimaryKey(
    opts.datasources.find((d) => d.name === entitySnake),
    opts,
  );
  const normalizedByFields = normalizeByFields(candidate.byFields);
  const tokens = {
    serviceImport: naming.serviceUseLine(entitySnake, className),
    className,
    entitySnake,
    path,
    idTypeVariant: idTypeVariantForPk(pk),
    hasByFields: normalizedByFields.length > 0,
    byFieldMerges: byFieldMergeChain(
      entitySnake,
      naming.apiPath(entitySnake),
      normalizedByFields,
      false,
    ),
  };
  return content(naming.filePath(entitySnake), fill(readonlyTmpl, tokens));
};

const renderCrud = (
  candidate: RouteCandidate,
  opts: EmitOptions,
): GenerateEntry => {
  const { naming } = opts;
  const entitySnake = candidate.name;
  const className = naming.serviceClassName(entitySnake);
  const path = `/api/${naming.apiPath(entitySnake)}`;
  const allFields = fieldsForEntity(entitySnake, opts.datasources);
  const enrichments = enrichmentsForEntity(entitySnake, opts.views);
  const eagerWriteChildren = eagerWriteChildrenForEntity(
    entitySnake,
    opts.nested,
  );
  const pk = inferPrimaryKey(
    opts.datasources.find((d) => d.name === entitySnake),
    opts,
  );
  const normalizedByFields = normalizeByFields(candidate.byFields);
  const hasByFields = normalizedByFields.length > 0;
  const directFkChildren = sortedDirectFkChildren(eagerWriteChildren);
  const { booleanFields, binaryFields } = partitionFieldsByType(allFields);
  const hasCoercion = booleanFields.length + binaryFields.length > 0;
  const coerceFn = hasCoercion
    ? `\n\n${generateCoerceRowFn(booleanFields, binaryFields)}`
    : "";
  const { createValidator, updateValidator } = buildCrudValidators(
    entitySnake,
    applyEnrichmentToRequiredFields([], enrichments),
    directFkChildren,
  );
  const occ = entityUsesOptimisticConcurrency(
    {
      datasourceType: candidate.datasourceType,
      optimisticConcurrency: candidate.optimisticConcurrency,
    },
    opts.useOptimisticConcurrency,
  );
  const tokens = {
    serviceImport: naming.serviceUseLine(entitySnake, className),
    className,
    entitySnake,
    path,
    idTypeVariant: idTypeVariantForPk(pk),
    primaryKeyParamExpr:
      pk.column === "id" ? "None" : `Some("${pk.column}".to_string())`,
    useOptimisticConcurrency: occ ? "true" : "false",
    coerceRowExpr: hasCoercion
      ? "Some(Arc::new(|row: &mut RowMap| coerce_row_types(row)))"
      : "None",
    hasCoercion,
    hasByFields,
    createValidator,
    updateValidator,
    coerceFn,
    byFieldMerges: byFieldMergeChain(
      entitySnake,
      naming.apiPath(entitySnake),
      normalizedByFields,
      hasCoercion,
    ),
  };
  return content(naming.filePath(entitySnake), fill(crudTmpl, tokens));
};

const renderEntityRouter = (
  candidate: RouteCandidate,
  opts: EmitOptions,
): GenerateEntry =>
  candidate.datasourceType === "readonly-lookup"
    ? renderReadOnly(candidate, opts)
    : renderCrud(candidate, opts);

const renderAppWiring = (
  candidates: RouteCandidate[],
  opts: EmitOptions,
): GenerateEntry => {
  const { naming } = opts;
  const services = candidates.map((c) => ({
    fieldName: serviceFieldName(c.name),
    className: naming.serviceClassName(c.name),
    routeModule: naming.routeModulePath(c.name),
  }));
  return content(
    naming.appWiringFilePath(),
    fill(appWiringTmpl, {
      imports: candidates.map((c) =>
        naming.serviceUseLine(c.name, naming.serviceClassName(c.name)),
      ),
      body: fill(appWiringBodyTmpl, {
        hasServices: services.length > 0,
        services,
      }).trimEnd(),
    }),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const parser = new SpecificationParser(ctx.reader);
  const parsed = await parser.loadRoutes({
    idType: ctx.settings["datasource.id_type"] ?? "integer",
  });
  const views = (await ctx.reader.exists("view_types.yaml"))
    ? await parser.loadViewTypes()
    : [];
  const opts = await emitOptions(
    ctx.settings,
    views,
    parsed.nested,
    parsed.datasources,
  );
  const entries: GenerateEntry[] = parsed.candidates.map((c) =>
    renderEntityRouter(c, opts),
  );
  if (parsed.candidates.length > 0) {
    entries.push(renderAppWiring(parsed.candidates, opts));
  }
  return entries;
};
