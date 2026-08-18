import pluralize from "pluralize";
import { datasourceSettings } from "./common/datasource-settings.ts";
import { fill } from "./common/fill.ts";
import type { GenerateContext, SettingsDict } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import {
  rustRouteNaming,
  type RouteNaming,
} from "./common/naming.ts";
import {
  entityUsesOptimisticConcurrency,
  loadRoutes,
  type NestedRouteDescriptor,
  type RouteByField,
  type RouteCandidate,
} from "./common/parse-routes.ts";
import type { DatasourceField, DatasourceType } from "./common/parse-datasource-types.ts";
import {
  loadViewTypes,
  type ViewEnrichment,
  type ViewType,
} from "./common/parse-view-types.ts";
import {
  appWiringTmpl,
  crudByFieldsTmpl,
  crudPlainTmpl,
  readonlyByFieldsTmpl,
  readonlyPlainTmpl,
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
  naming: RouteNaming;
  idType: string;
  rustIdType: string;
  useOptimisticConcurrency: boolean;
  views: ViewType[];
  datasources: DatasourceType[];
  nested: NestedRouteDescriptor[];
};

const emitOptions = async (
  settings: SettingsDict,
  reader: GenerateContext["reader"],
  nested: NestedRouteDescriptor[],
  datasources: DatasourceType[],
): Promise<EmitOptions> => {
  const ds = datasourceSettings(settings);
  const hasViews = await reader.exists("view_types.yaml");
  const views = hasViews ? await loadViewTypes(reader) : [];
  return {
    naming: rustRouteNaming(settings),
    idType: ds.idType,
    rustIdType: ds.rustIdType,
    useOptimisticConcurrency: ds.useOptimisticConcurrency,
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

const byFieldPrimitiveCall = (
  entitySnake: string,
  apiPath: string,
  entry: NormalizedByField,
  hasCoercion: boolean,
): string => {
  const coerceExpr = hasCoercion
    ? "Some(Arc::new(|row: &mut RowMap| coerce_row_types(row)))"
    : "None";
  const path = `/api/${apiPath}`;
  return `create_by_field_router(ByFieldRouterConfig {
        service: service.clone(),
        entity_name: "${entitySnake}".to_string(),
        base_path: "${path}".to_string(),
        field: "${entry.byField}".to_string(),
        unique: ${entry.unique ? "true" : "false"},
        methods: vec![deterministic::routes::ByFieldMethod::Get],
        coerce_row: ${coerceExpr},
        update_validator: None,
    })`;
};

const byFieldMergeChain = (
  entitySnake: string,
  apiPath: string,
  normalizedByFields: NormalizedByField[],
  hasCoercion: boolean,
): string =>
  normalizedByFields
    .map(
      (e) =>
        `    .merge(${byFieldPrimitiveCall(entitySnake, apiPath, e, hasCoercion)})`,
    )
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

const buildEagerChildShapeCheck = (child: EagerChild): string => {
  const name = child.fieldName;
  return `    match body.get(${JSON.stringify(name)}) {
        None => {}
        Some(v) if v.is_null() => {}
        Some(v) => match v {
            serde_json::Value::Array(arr) => {
                for (i, item) in arr.iter().enumerate() {
                    if !item.is_object() {
                        errors.push(format!("${name}[{}]: expected object", i));
                    }
                }
            }
            _ => errors.push("${name}: expected array".to_string()),
        }
    }`;
};

const requiredFieldCheck = (f: Field, requireAll: boolean): string => {
  const typeCheck = fieldTypeCheck(f.type);
  const present = `body.get("${f.name}").filter(|v| !v.is_null())`;
  if (requireAll) {
    if (typeCheck) {
      return `    match ${present} {
        None => errors.push("${f.name}: required".to_string()),
        Some(v) if !v.${typeCheck}() => errors.push("${f.name}: expected ${f.type}".to_string()),
        _ => {}
    }`;
    }
    return `    if ${present}.is_none() { errors.push("${f.name}: required".to_string()); }`;
  }
  if (typeCheck) {
    return `    if let Some(v) = ${present} {
        if !v.${typeCheck}() { errors.push("${f.name}: expected ${f.type}".to_string()); }
    }`;
  }
  return "";
};

const buildValidatorFn = (args: {
  fnName: string;
  requiredFields: Field[];
  requireAll: boolean;
  directFkChildren: EagerChild[];
}): string => {
  const requiredChecks = args.requiredFields
    .map((f) => requiredFieldCheck(f, args.requireAll))
    .filter((s) => s.length > 0);
  const childShapeChecks = args.directFkChildren.map(buildEagerChildShapeCheck);
  const checks = [...requiredChecks, ...childShapeChecks].join("\n");
  if (checks.length === 0) {
    return `fn ${args.fnName}(_body: &RowMap) -> Result<(), Vec<String>> { Ok(()) }`;
  }
  return `fn ${args.fnName}(body: &RowMap) -> Result<(), Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
${checks}
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}`;
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
): string => {
  const boolNames = booleanFields.map((f) => f.name);
  const binNames = binaryFields.map((f) => f.name);
  const boolArr = boolNames.length
    ? `&[${boolNames.map((n) => JSON.stringify(n)).join(", ")}]`
    : "&[]";
  const binArr = binNames.length
    ? `&[${binNames.map((n) => JSON.stringify(n)).join(", ")}]`
    : "&[]";
  return `fn coerce_row_types(row: &mut RowMap) {
    let bool_cols: &[&str] = ${boolArr};
    let binary_cols: &[&str] = ${binArr};
    for col in bool_cols {
        if let Some(v) = row.get(*col).cloned() {
            match v {
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        row.insert((*col).to_string(), Value::Bool(i != 0));
                    }
                }
                Value::Bool(_) | Value::Null => {}
                _ => {}
            }
        }
    }
    for col in binary_cols {
        if let Some(v) = row.get(*col).cloned() {
            match v {
                Value::Array(arr) => {
                    let bytes: Vec<u8> = arr
                        .iter()
                        .filter_map(|x| x.as_u64().and_then(|n| u8::try_from(n).ok()))
                        .collect();
                    let encoded = base64_encode(&bytes);
                    row.insert((*col).to_string(), Value::String(encoded));
                }
                Value::Null | Value::String(_) => {}
                _ => {}
            }
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(CHARSET[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 12) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 6) & 0x3f) as usize] as char);
        out.push(CHARSET[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(CHARSET[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(CHARSET[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 12) & 0x3f) as usize] as char);
        out.push(CHARSET[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}`;
};

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
    byFieldMerges: byFieldMergeChain(
      entitySnake,
      naming.apiPath(entitySnake),
      normalizedByFields,
      false,
    ),
  };
  const tmpl =
    normalizedByFields.length > 0 ? readonlyByFieldsTmpl : readonlyPlainTmpl;
  return content(naming.filePath(entitySnake), fill(tmpl, tokens));
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
  const tmpl = hasByFields ? crudByFieldsTmpl : crudPlainTmpl;
  return content(naming.filePath(entitySnake), fill(tmpl, tokens));
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
  const imports = candidates.map((c) =>
    naming.serviceUseLine(c.name, naming.serviceClassName(c.name)),
  );
  const lets = candidates.map(
    (c) =>
      `    let ${serviceFieldName(c.name)} = std::sync::Arc::new(${naming.serviceClassName(c.name)}::from_context(ctx)?);`,
  );
  const merges = candidates.map(
    (c) =>
      `        .merge(${naming.routeModulePath(c.name)}::router(${serviceFieldName(c.name)}.clone()))`,
  );
  const body =
    merges.length > 0
      ? `${lets.join("\n")}${lets.length > 0 ? "\n\n" : ""}    let router = axum::Router::new()\n${merges.join("\n")};\n    Ok(router)`
      : `    Ok(axum::Router::new())`;
  return content(
    naming.appWiringFilePath(),
    fill(appWiringTmpl, { imports, body }),
  );
};

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  const ds = datasourceSettings(ctx.settings);
  const parsed = await loadRoutes(ctx.reader, { idType: ds.idType });
  const opts = await emitOptions(
    ctx.settings,
    ctx.reader,
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
