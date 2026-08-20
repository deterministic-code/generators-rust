use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::dynamic_service::{DynamicService, ServiceError};
use super::IF_MATCH_ARG;
use crate::error::RepositoryError;
use crate::loaders::eager_write::{BindingKind, EagerWriteChildBinding};
use crate::repositories::{CrudRepository, Datasource};

pub type ServiceFactory = Arc<
    dyn Fn(Arc<dyn Datasource>) -> Result<Arc<dyn DynamicService>, RepositoryError> + Send + Sync,
>;

pub type CrudRepoFactory = Arc<
    dyn Fn(Arc<dyn Datasource>) -> Result<Arc<dyn CrudRepository>, RepositoryError> + Send + Sync,
>;

#[derive(Clone)]
pub struct EagerWriteChildBindingRuntime {
    pub binding: EagerWriteChildBinding,
    pub child_service_factory: ServiceFactory,
    // why optional: only m2m bindings need a junction repo; direct-fk bindings leave this None.
    pub junction_repo_factory: Option<CrudRepoFactory>,
    pub children: Vec<EagerWriteChildBindingRuntime>,
}

pub struct EagerChildWritingService {
    base: Arc<dyn DynamicService>,
    pool_datasource: Arc<dyn Datasource>,
    parent_service_factory: ServiceFactory,
    parent_pk_column: String,
    bindings: Vec<EagerWriteChildBindingRuntime>,
}

impl EagerChildWritingService {
    pub fn new(
        base: Arc<dyn DynamicService>,
        pool_datasource: Arc<dyn Datasource>,
        parent_service_factory: ServiceFactory,
        parent_pk_column: impl Into<String>,
        bindings: Vec<EagerWriteChildBindingRuntime>,
    ) -> Self {
        Self {
            base,
            pool_datasource,
            parent_service_factory,
            parent_pk_column: parent_pk_column.into(),
            bindings,
        }
    }
}

// why ServiceError → RepositoryError: run_in_transaction returns RepositoryError; we squeeze ServiceError into Other so rollback fires and the caller still sees the message.
fn svc_err_to_repo(e: ServiceError) -> RepositoryError {
    RepositoryError::Other(format!("service error: {}", e))
}

// why child If-Match from the loaded row: internal reconciliation updates/deletes OCC-guarded child
// rows within the parent's transaction, but the client only sends the parent's If-Match. The eager
// write owns its children and just loaded their current `updated`, so it passes that as the child
// token — honest OCC (expects the version it just read), not a bypass. No-op when the child carries
// no `updated` (not version-tracked).
fn with_child_if_match(mut args: Value, existing_child: &Value) -> Value {
    if let (Value::Object(map), Some(updated)) = (
        &mut args,
        existing_child.get("updated").and_then(|v| v.as_str()),
    ) {
        map.insert(IF_MATCH_ARG.to_string(), Value::String(updated.to_string()));
    }
    args
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Create,
    Update,
    Patch,
}

#[async_trait]
impl DynamicService for EagerChildWritingService {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
        match method {
            "findAll" | "find_all" | "findBy" | "find_by" | "findById" | "find_by_id" | "find"
            | "query" => self.base.invoke(method, args).await,
            "create" | "add" => self.run_create(args).await,
            "update" => self.run_update(args, Mode::Update).await,
            "patch" => self.run_update(args, Mode::Patch).await,
            "delete" => self.run_delete(args).await,
            other => self.base.invoke(other, args).await,
        }
    }
}

impl EagerChildWritingService {
    async fn run_create(&self, args: Value) -> Result<Value, ServiceError> {
        let body = extract_body(args);
        validate_rows_recursively(&body, &self.bindings, Mode::Create)?;
        let (parent_data, nested) = split_row(&body, &self.bindings);

        let parent_factory = self.parent_service_factory.clone();
        let parent_pk_column = self.parent_pk_column.clone();
        let bindings = self.bindings.clone();

        let result = self
            .pool_datasource
            .run_in_transaction(Box::new(move |txn_ds| {
                Box::pin(async move {
                    let parent_svc = (parent_factory)(txn_ds.clone()).map_err(|e| e)?;
                    let created_parent = parent_svc
                        .invoke("create", Value::Object(parent_data))
                        .await
                        .map_err(svc_err_to_repo)?;
                    let parent_id = pluck_id(&created_parent, &parent_pk_column)?;
                    let mut response = match created_parent {
                        Value::Object(map) => map,
                        _ => Map::new(),
                    };
                    let reconciled = process_bindings_for_parent(
                        &bindings,
                        &parent_id,
                        &nested,
                        &txn_ds,
                        Mode::Create,
                    )
                    .await?;
                    for (k, v) in reconciled {
                        response.insert(k, v);
                    }
                    Ok(Value::Object(response))
                })
            }))
            .await
            .map_err(repo_err_to_svc)?;
        Ok(result)
    }

    async fn run_update(&self, args: Value, mode: Mode) -> Result<Value, ServiceError> {
        let id = args
            .get("id")
            .cloned()
            .ok_or_else(|| ServiceError::InvalidArgs("missing `id`".to_string()))?;
        let if_match = args
            .get(IF_MATCH_ARG)
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let body = extract_body(args);
        validate_rows_recursively(&body, &self.bindings, mode)?;
        let (parent_data, nested) = split_row(&body, &self.bindings);

        let parent_factory = self.parent_service_factory.clone();
        let parent_pk_column = self.parent_pk_column.clone();
        let bindings = self.bindings.clone();
        let id_clone = id.clone();

        let result = self
            .pool_datasource
            .run_in_transaction(Box::new(move |txn_ds| {
                Box::pin(async move {
                    let parent_svc = (parent_factory)(txn_ds.clone())?;
                    let existing = parent_svc
                        .invoke("findById", json!({ "id": id_clone.clone() }))
                        .await
                        .map_err(svc_err_to_repo)?;
                    if existing.is_null() {
                        return Ok(Value::Null);
                    }
                    let updated_parent = if parent_data.is_empty() {
                        existing
                    } else {
                        let mut update_args =
                            json!({ "id": id_clone.clone(), "data": Value::Object(parent_data) });
                        if let (Some(m), Value::Object(map)) = (&if_match, &mut update_args) {
                            map.insert(IF_MATCH_ARG.to_string(), Value::String(m.clone()));
                        }
                        let updated = parent_svc
                            .invoke("update", update_args)
                            .await
                            .map_err(svc_err_to_repo)?;
                        if updated.is_null() {
                            return Ok(Value::Null);
                        }
                        updated
                    };
                    let parent_id = pluck_id(&updated_parent, &parent_pk_column)?;
                    let mut response = match updated_parent {
                        Value::Object(map) => map,
                        _ => Map::new(),
                    };
                    let reconciled =
                        process_bindings_for_parent(&bindings, &parent_id, &nested, &txn_ds, mode)
                            .await?;
                    for (k, v) in reconciled {
                        response.insert(k, v);
                    }
                    Ok(Value::Object(response))
                })
            }))
            .await
            .map_err(repo_err_to_svc)?;
        Ok(result)
    }

    async fn run_delete(&self, args: Value) -> Result<Value, ServiceError> {
        let id = args
            .get("id")
            .cloned()
            .ok_or_else(|| ServiceError::InvalidArgs("missing `id`".to_string()))?;
        let if_match = args
            .get(IF_MATCH_ARG)
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let parent_factory = self.parent_service_factory.clone();
        let bindings = self.bindings.clone();

        let result = self
            .pool_datasource
            .run_in_transaction(Box::new(move |txn_ds| {
                Box::pin(async move {
                    delete_children_for_parent(&bindings, &id, &txn_ds).await?;
                    let parent_svc = (parent_factory)(txn_ds.clone())?;
                    let mut delete_args = json!({ "id": id });
                    if let (Some(m), Value::Object(map)) = (&if_match, &mut delete_args) {
                        map.insert(IF_MATCH_ARG.to_string(), Value::String(m.clone()));
                    }
                    let deleted = parent_svc
                        .invoke("delete", delete_args)
                        .await
                        .map_err(svc_err_to_repo)?;
                    Ok(deleted)
                })
            }))
            .await
            .map_err(repo_err_to_svc)?;
        Ok(result)
    }
}

fn repo_err_to_svc(e: RepositoryError) -> ServiceError {
    ServiceError::Repository(e)
}

fn extract_body(args: Value) -> Map<String, Value> {
    match args {
        Value::Object(mut map) => {
            if let Some(Value::Object(body)) = map.remove("body") {
                return body;
            }
            if let Some(Value::Object(data)) = map.remove("data") {
                return data;
            }
            map.remove("id");
            map.remove("query");
            map
        }
        _ => Map::new(),
    }
}

fn pluck_id(row: &Value, pk_column: &str) -> Result<Value, RepositoryError> {
    row.get(pk_column)
        .cloned()
        .ok_or_else(|| RepositoryError::Other(format!("created row missing `{}`", pk_column)))
}

fn split_row(
    row: &Map<String, Value>,
    bindings: &[EagerWriteChildBindingRuntime],
) -> (Map<String, Value>, Map<String, Value>) {
    let mut base = row.clone();
    let mut nested = Map::new();
    for b in bindings {
        if let Some(v) = base.remove(&b.binding.field_name) {
            nested.insert(b.binding.field_name.clone(), v);
        }
    }
    (base, nested)
}

fn incoming_child_rows(
    parent_row: &Map<String, Value>,
    b: &EagerWriteChildBindingRuntime,
) -> Option<Vec<Value>> {
    b.binding
        .unpack_incoming(parent_row.get(&b.binding.field_name))
}

fn validate_rows_recursively(
    parent_row: &Map<String, Value>,
    bindings: &[EagerWriteChildBindingRuntime],
    mode: Mode,
) -> Result<(), ServiceError> {
    for b in bindings {
        let Some(arr) = incoming_child_rows(parent_row, b) else {
            continue;
        };
        for row in arr {
            let Value::Object(obj) = row else { continue };
            let is_m2m = matches!(&b.binding.kind, BindingKind::M2m { .. });
            if mode == Mode::Create && !is_m2m && obj.contains_key("id") {
                return Err(ServiceError::InvalidArgs(
                    "nested row cannot have id field".to_string(),
                ));
            }
            if matches!(mode, Mode::Update | Mode::Patch) {
                if let Some(idv) = obj.get("id") {
                    if idv.is_null() {
                        return Err(ServiceError::InvalidArgs(
                            "id field cannot be null when provided".to_string(),
                        ));
                    }
                }
            }
            if !b.children.is_empty() {
                validate_rows_recursively(&obj, &b.children, mode)?;
            }
        }
    }
    Ok(())
}

// why boxed: recursive async fns can't be plain async without boxing the future.
fn process_bindings_for_parent<'a>(
    bindings: &'a [EagerWriteChildBindingRuntime],
    parent_id: &'a Value,
    parent_incoming_row: &'a Map<String, Value>,
    txn_ds: &'a Arc<dyn Datasource>,
    mode: Mode,
) -> Pin<Box<dyn Future<Output = Result<Map<String, Value>, RepositoryError>> + Send + 'a>> {
    Box::pin(async move {
        let mut out = Map::new();
        for b in bindings {
            let child_rows = b
                .binding
                .unpack_incoming(parent_incoming_row.get(&b.binding.field_name));
            if mode == Mode::Create {
                let arr = match child_rows {
                    Some(rows) => create_child_array(b, parent_id, rows, txn_ds).await?,
                    None => Vec::new(),
                };
                out.insert(b.binding.field_name.clone(), b.binding.pack_children(arr));
                continue;
            }
            // update / patch
            let arr = match child_rows {
                Some(rows) => reconcile_child_array(b, parent_id, rows, txn_ds, mode).await?,
                None => load_existing_for(b, parent_id, txn_ds).await?,
            };
            out.insert(b.binding.field_name.clone(), b.binding.pack_children(arr));
        }
        Ok(out)
    })
}

fn create_child_array<'a>(
    binding: &'a EagerWriteChildBindingRuntime,
    parent_id: &'a Value,
    child_rows: Vec<Value>,
    txn_ds: &'a Arc<dyn Datasource>,
) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, RepositoryError>> + Send + 'a>> {
    Box::pin(async move {
        let nested = &binding.children;
        match &binding.binding.kind {
            BindingKind::M2m {
                parent_fk_column,
                target_fk_column,
                ..
            } => {
                let target_svc = (binding.child_service_factory)(txn_ds.clone())?;
                let junction_repo = (binding.junction_repo_factory.as_ref().ok_or_else(|| {
                    RepositoryError::Other("missing junction repo factory".into())
                })?)(txn_ds.clone())?;
                let mut result = Vec::new();
                for row in child_rows {
                    let Value::Object(obj) = row else { continue };
                    let (base, nested_fields) = split_row_value(obj, nested);
                    let target_id = resolve_or_create_target_id(base, &target_svc).await?;
                    let mut junction_row = crate::repositories::RowMap::new();
                    junction_row.insert(parent_fk_column.clone(), parent_id.clone());
                    junction_row.insert(target_fk_column.clone(), target_id.clone());
                    junction_repo.add(junction_row).await?;
                    let target = target_svc
                        .invoke("findById", json!({ "id": target_id.clone() }))
                        .await
                        .map_err(svc_err_to_repo)?;
                    if target.is_null() {
                        continue;
                    }
                    let mut enriched = match target {
                        Value::Object(m) => m,
                        other => {
                            result.push(other);
                            continue;
                        }
                    };
                    if !nested.is_empty() {
                        let nested_recon = process_bindings_for_parent(
                            nested,
                            &target_id,
                            &nested_fields,
                            txn_ds,
                            Mode::Create,
                        )
                        .await?;
                        for (k, v) in nested_recon {
                            enriched.insert(k, v);
                        }
                    }
                    result.push(Value::Object(enriched));
                }
                Ok(result)
            }
            BindingKind::DirectFk { fk_column } => {
                let child_svc = (binding.child_service_factory)(txn_ds.clone())?;
                let mut result = Vec::new();
                for row in child_rows {
                    let Value::Object(obj) = row else { continue };
                    let (mut base, nested_fields) = split_row_value(obj, nested);
                    base.insert(fk_column.clone(), parent_id.clone());
                    let created = child_svc
                        .invoke("create", Value::Object(base))
                        .await
                        .map_err(svc_err_to_repo)?;
                    let child_id = created.get("id").cloned().ok_or_else(|| {
                        RepositoryError::Other("created child missing `id`".into())
                    })?;
                    let mut enriched = match created {
                        Value::Object(m) => m,
                        other => {
                            result.push(other);
                            continue;
                        }
                    };
                    if !nested.is_empty() {
                        let nested_recon = process_bindings_for_parent(
                            nested,
                            &child_id,
                            &nested_fields,
                            txn_ds,
                            Mode::Create,
                        )
                        .await?;
                        for (k, v) in nested_recon {
                            enriched.insert(k, v);
                        }
                    }
                    result.push(Value::Object(enriched));
                }
                Ok(result)
            }
        }
    })
}

fn reconcile_child_array<'a>(
    binding: &'a EagerWriteChildBindingRuntime,
    parent_id: &'a Value,
    child_rows: Vec<Value>,
    txn_ds: &'a Arc<dyn Datasource>,
    mode: Mode,
) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, RepositoryError>> + Send + 'a>> {
    Box::pin(async move {
        let nested = &binding.children;
        match &binding.binding.kind {
            BindingKind::M2m {
                parent_fk_column,
                target_fk_column,
                ..
            } => {
                let target_svc = (binding.child_service_factory)(txn_ds.clone())?;
                let junction_repo = (binding.junction_repo_factory.as_ref().ok_or_else(|| {
                    RepositoryError::Other("missing junction repo factory".into())
                })?)(txn_ds.clone())?;
                let existing_junctions = junction_repo.find_by(parent_fk_column, parent_id).await?;
                let existing_target_ids: Vec<Value> = existing_junctions
                    .iter()
                    .filter_map(|j| j.get(target_fk_column).cloned())
                    .collect();

                let mut resolved: Vec<(Value, Value, Map<String, Value>)> = Vec::new();
                let mut incoming_target_ids: Vec<Value> = Vec::new();
                for row in child_rows {
                    let Value::Object(obj) = row else { continue };
                    let (base, nested_fields) = split_row_value(obj, nested);
                    let target_id = resolve_or_create_target_id(base, &target_svc).await?;
                    incoming_target_ids.push(target_id.clone());
                    if !existing_target_ids.iter().any(|v| values_eq(v, &target_id)) {
                        let mut junction_row = crate::repositories::RowMap::new();
                        junction_row.insert(parent_fk_column.clone(), parent_id.clone());
                        junction_row.insert(target_fk_column.clone(), target_id.clone());
                        junction_repo.add(junction_row).await?;
                    }
                    resolved.push((target_id.clone(), target_id, nested_fields));
                }

                for j in &existing_junctions {
                    let Some(tid) = j.get(target_fk_column) else {
                        continue;
                    };
                    if incoming_target_ids.iter().any(|v| values_eq(v, tid)) {
                        continue;
                    }
                    if let Some(jid) = j.get("id") {
                        junction_repo.delete(jid).await?;
                    }
                }

                let mut result = Vec::new();
                for (_orig, target_id, nested_fields) in resolved {
                    let target = target_svc
                        .invoke("findById", json!({ "id": target_id.clone() }))
                        .await
                        .map_err(svc_err_to_repo)?;
                    if target.is_null() {
                        continue;
                    }
                    let mut enriched = match target {
                        Value::Object(m) => m,
                        other => {
                            result.push(other);
                            continue;
                        }
                    };
                    if !nested.is_empty() {
                        let nested_recon = process_bindings_for_parent(
                            nested,
                            &target_id,
                            &nested_fields,
                            txn_ds,
                            mode,
                        )
                        .await?;
                        for (k, v) in nested_recon {
                            enriched.insert(k, v);
                        }
                    }
                    result.push(Value::Object(enriched));
                }
                Ok(result)
            }
            BindingKind::DirectFk { fk_column } => {
                let child_svc = (binding.child_service_factory)(txn_ds.clone())?;
                let existing = child_svc
                    .invoke(
                        "findBy",
                        json!({ "column": fk_column.clone(), "value": parent_id.clone() }),
                    )
                    .await
                    .map_err(svc_err_to_repo)?;
                let existing_arr: Vec<Value> = match existing {
                    Value::Array(a) => a,
                    _ => Vec::new(),
                };

                enum Op {
                    Update {
                        id: Value,
                        data: Map<String, Value>,
                        nested_fields: Map<String, Value>,
                    },
                    Create {
                        data: Map<String, Value>,
                        nested_fields: Map<String, Value>,
                    },
                }
                let mut ops: Vec<Op> = Vec::new();
                let mut incoming_ids: Vec<Value> = Vec::new();
                for row in child_rows {
                    let Value::Object(obj) = row else { continue };
                    let (mut base, nested_fields) = split_row_value(obj, nested);
                    let child_id_opt = base.remove("id").filter(|v| !v.is_null());
                    if let Some(child_id) = child_id_opt {
                        let belongs = existing_arr.iter().any(|c| {
                            c.get("id")
                                .map(|v| values_eq(v, &child_id))
                                .unwrap_or(false)
                        });
                        if !belongs {
                            return Err(RepositoryError::Other(format!(
                                "cross-parent guard: child id {} does not belong to this parent",
                                child_id
                            )));
                        }
                        incoming_ids.push(child_id.clone());
                        ops.push(Op::Update {
                            id: child_id,
                            data: base,
                            nested_fields,
                        });
                    } else {
                        base.insert(fk_column.clone(), parent_id.clone());
                        ops.push(Op::Create {
                            data: base,
                            nested_fields,
                        });
                    }
                }

                for existing_child in &existing_arr {
                    let Some(eid) = existing_child.get("id") else {
                        continue;
                    };
                    if incoming_ids.iter().any(|v| values_eq(v, eid)) {
                        continue;
                    }
                    // why cascade before delete: child_svc is the bare per-entity service (not eager-wrapped), so DELETE FROM <child> alone leaves m2m junctions / direct-fk grandchildren orphaned and trips FK constraints — sqlite code 787 on user-PATCH with posts that had tags.
                    if !nested.is_empty() {
                        delete_children_for_parent(nested, eid, txn_ds).await?;
                    }
                    child_svc
                        .invoke(
                            "delete",
                            with_child_if_match(json!({ "id": eid.clone() }), existing_child),
                        )
                        .await
                        .map_err(svc_err_to_repo)?;
                }

                let mut result = Vec::new();
                for op in ops {
                    let (child_row, child_id, nested_fields) = match op {
                        Op::Update {
                            id,
                            data,
                            nested_fields,
                        } => {
                            let mut update_args =
                                json!({ "id": id.clone(), "data": Value::Object(data) });
                            if let Some(existing_child) = existing_arr
                                .iter()
                                .find(|c| c.get("id").map(|v| values_eq(v, &id)).unwrap_or(false))
                            {
                                update_args = with_child_if_match(update_args, existing_child);
                            }
                            let row = child_svc
                                .invoke("update", update_args)
                                .await
                                .map_err(svc_err_to_repo)?;
                            (row, id, nested_fields)
                        }
                        Op::Create {
                            data,
                            nested_fields,
                        } => {
                            let row = child_svc
                                .invoke("create", Value::Object(data))
                                .await
                                .map_err(svc_err_to_repo)?;
                            let id = row.get("id").cloned().ok_or_else(|| {
                                RepositoryError::Other("created child missing `id`".into())
                            })?;
                            (row, id, nested_fields)
                        }
                    };
                    if child_row.is_null() {
                        continue;
                    }
                    let mut enriched = match child_row {
                        Value::Object(m) => m,
                        other => {
                            result.push(other);
                            continue;
                        }
                    };
                    if !nested.is_empty() {
                        let nested_recon = process_bindings_for_parent(
                            nested,
                            &child_id,
                            &nested_fields,
                            txn_ds,
                            mode,
                        )
                        .await?;
                        for (k, v) in nested_recon {
                            enriched.insert(k, v);
                        }
                    }
                    result.push(Value::Object(enriched));
                }
                Ok(result)
            }
        }
    })
}

/// Load one binding's children for a parent id (direct-fk or m2m, with nested recursion), for the
/// read side. Shared with [`crate::services::EagerChildReadingService`] so eager reads and the
/// eager-write update path resolve children through one code path. `ds` may be the pool datasource
/// (plain read) or a txn datasource — the child service factories work with either.
pub(crate) async fn load_binding_children(
    binding: &EagerWriteChildBindingRuntime,
    parent_id: &Value,
    ds: &Arc<dyn Datasource>,
) -> Result<Vec<Value>, RepositoryError> {
    load_existing_for(binding, parent_id, ds).await
}

fn load_existing_for<'a>(
    binding: &'a EagerWriteChildBindingRuntime,
    parent_id: &'a Value,
    txn_ds: &'a Arc<dyn Datasource>,
) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, RepositoryError>> + Send + 'a>> {
    Box::pin(async move {
        let nested = &binding.children;
        match &binding.binding.kind {
            BindingKind::M2m {
                parent_fk_column,
                target_fk_column,
                ..
            } => {
                let target_svc = (binding.child_service_factory)(txn_ds.clone())?;
                let junction_repo = (binding.junction_repo_factory.as_ref().ok_or_else(|| {
                    RepositoryError::Other("missing junction repo factory".into())
                })?)(txn_ds.clone())?;
                let junction_rows = junction_repo.find_by(parent_fk_column, parent_id).await?;
                let mut out = Vec::new();
                for j in junction_rows {
                    let Some(target_id) = j.get(target_fk_column).cloned() else {
                        continue;
                    };
                    let target = target_svc
                        .invoke("findById", json!({ "id": target_id.clone() }))
                        .await
                        .map_err(svc_err_to_repo)?;
                    if target.is_null() {
                        continue;
                    }
                    let mut enriched = match target {
                        Value::Object(m) => m,
                        other => {
                            out.push(other);
                            continue;
                        }
                    };
                    if !nested.is_empty() {
                        let nested_recon = process_bindings_for_parent(
                            nested,
                            &target_id,
                            &Map::new(),
                            txn_ds,
                            Mode::Update,
                        )
                        .await?;
                        for (k, v) in nested_recon {
                            enriched.insert(k, v);
                        }
                    }
                    out.push(Value::Object(enriched));
                }
                Ok(out)
            }
            BindingKind::DirectFk { fk_column } => {
                let child_svc = (binding.child_service_factory)(txn_ds.clone())?;
                let existing = child_svc
                    .invoke(
                        "findBy",
                        json!({ "column": fk_column.clone(), "value": parent_id.clone() }),
                    )
                    .await
                    .map_err(svc_err_to_repo)?;
                let arr = match existing {
                    Value::Array(a) => a,
                    _ => Vec::new(),
                };
                if nested.is_empty() {
                    return Ok(arr);
                }
                let mut out = Vec::new();
                for child in arr {
                    let Some(child_id) = child.get("id").cloned() else {
                        out.push(child);
                        continue;
                    };
                    let mut enriched = match child {
                        Value::Object(m) => m,
                        other => {
                            out.push(other);
                            continue;
                        }
                    };
                    let nested_recon = process_bindings_for_parent(
                        nested,
                        &child_id,
                        &Map::new(),
                        txn_ds,
                        Mode::Update,
                    )
                    .await?;
                    for (k, v) in nested_recon {
                        enriched.insert(k, v);
                    }
                    out.push(Value::Object(enriched));
                }
                Ok(out)
            }
        }
    })
}

fn delete_children_for_parent<'a>(
    bindings: &'a [EagerWriteChildBindingRuntime],
    parent_id: &'a Value,
    txn_ds: &'a Arc<dyn Datasource>,
) -> Pin<Box<dyn Future<Output = Result<(), RepositoryError>> + Send + 'a>> {
    Box::pin(async move {
        // why reverse: mirrors create order — FK dependencies require deletes to run inverse.
        for b in bindings.iter().rev() {
            match &b.binding.kind {
                BindingKind::M2m {
                    parent_fk_column, ..
                } => {
                    let junction_repo = (b.junction_repo_factory.as_ref().ok_or_else(|| {
                        RepositoryError::Other("missing junction repo factory".into())
                    })?)(txn_ds.clone())?;
                    let rows = junction_repo.find_by(parent_fk_column, parent_id).await?;
                    for j in rows.iter().rev() {
                        if let Some(jid) = j.get("id") {
                            junction_repo.delete(jid).await?;
                        }
                    }
                }
                BindingKind::DirectFk { fk_column } => {
                    let child_svc = (b.child_service_factory)(txn_ds.clone())?;
                    let existing = child_svc
                        .invoke(
                            "findBy",
                            json!({ "column": fk_column.clone(), "value": parent_id.clone() }),
                        )
                        .await
                        .map_err(svc_err_to_repo)?;
                    let arr = match existing {
                        Value::Array(a) => a,
                        _ => Vec::new(),
                    };
                    let nested = &b.children;
                    for child in arr.iter().rev() {
                        let Some(cid) = child.get("id").cloned() else {
                            continue;
                        };
                        if !nested.is_empty() {
                            delete_children_for_parent(nested, &cid, txn_ds).await?;
                        }
                        child_svc
                            .invoke("delete", with_child_if_match(json!({ "id": cid }), child))
                            .await
                            .map_err(svc_err_to_repo)?;
                    }
                }
            }
        }
        Ok(())
    })
}

async fn resolve_or_create_target_id(
    row: Map<String, Value>,
    target_svc: &Arc<dyn DynamicService>,
) -> Result<Value, RepositoryError> {
    if let Some(id) = row.get("id") {
        if !id.is_null() {
            return Ok(id.clone());
        }
    }
    let mut rest = row;
    rest.remove("id");
    let created = target_svc
        .invoke("create", Value::Object(rest))
        .await
        .map_err(svc_err_to_repo)?;
    created
        .get("id")
        .cloned()
        .ok_or_else(|| RepositoryError::Other("target row missing `id`".into()))
}

fn split_row_value(
    row: Map<String, Value>,
    bindings: &[EagerWriteChildBindingRuntime],
) -> (Map<String, Value>, Map<String, Value>) {
    let mut base = row;
    let mut nested = Map::new();
    for b in bindings {
        if let Some(v) = base.remove(&b.binding.field_name) {
            nested.insert(b.binding.field_name.clone(), v);
        }
    }
    (base, nested)
}

// why custom eq: sqlite Number returns f64; postgres returns i64. Compare both numeric forms loosely.
fn values_eq(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    match (a.as_i64(), b.as_i64()) {
        (Some(x), Some(y)) => return x == y,
        _ => {}
    }
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::eager_write::BindingKind;
    use crate::repositories::inmemory::InMemoryCrudRepository;
    use crate::repositories::sqlite::{
        SqliteCrudRepository, SqliteDatasource, SqliteDatasourceOptions,
    };
    use crate::services::GenericCrudService;

    async fn open_sqlite_with_schema() -> Arc<SqliteDatasource> {
        let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
        ds.open().await.unwrap();
        ds.query(
            "CREATE TABLE todo (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, is_done INTEGER)",
            &[],
        )
        .await
        .unwrap();
        ds.query(
            "CREATE TABLE task (id INTEGER PRIMARY KEY AUTOINCREMENT, todo_id INTEGER, description TEXT)",
            &[],
        )
        .await
        .unwrap();
        ds.query(
            "CREATE TABLE meeting (id INTEGER PRIMARY KEY AUTOINCREMENT, todo_id INTEGER, scheduled_at TEXT)",
            &[],
        )
        .await
        .unwrap();
        Arc::new(ds)
    }

    fn build_sqlite_service_factory(table: &str) -> ServiceFactory {
        let table = table.to_string();
        Arc::new(move |ds: Arc<dyn Datasource>| {
            let repo = Arc::new(SqliteCrudRepository::new(ds, &table)?);
            let svc: Arc<dyn DynamicService> = Arc::new(GenericCrudService::new(repo));
            Ok(svc)
        })
    }

    fn direct_fk_binding(field: &str, table: &str, fk: &str) -> EagerWriteChildBindingRuntime {
        EagerWriteChildBindingRuntime {
            binding: EagerWriteChildBinding {
                kind: BindingKind::DirectFk {
                    fk_column: fk.to_string(),
                },
                field_name: field.to_string(),
                child_table: table.to_string(),
                children: Vec::new(),
                is_array: true,
            },
            child_service_factory: build_sqlite_service_factory(table),
            junction_repo_factory: None,
            children: Vec::new(),
        }
    }

    #[tokio::test]
    async fn create_persists_parent_and_direct_fk_children() {
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        let base_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![
                direct_fk_binding("tasks", "task", "todo_id"),
                direct_fk_binding("meetings", "meeting", "todo_id"),
            ],
        );

        let result = svc
            .invoke(
                "create",
                json!({
                    "body": {
                        "title": "demo",
                        "is_done": false,
                        "tasks": [
                            {"description": "a"},
                            {"description": "b"},
                        ],
                        "meetings": [{}],
                    }
                }),
            )
            .await
            .unwrap();

        assert_eq!(result["title"], json!("demo"));
        assert!(result["id"].is_number());
        let tasks = result["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 2);
        let parent_id = result["id"].as_i64().unwrap();
        for t in tasks {
            assert_eq!(t["todo_id"].as_i64().unwrap(), parent_id);
        }
        let descriptions: Vec<&str> = tasks
            .iter()
            .map(|t| t["description"].as_str().unwrap())
            .collect();
        assert_eq!(descriptions, vec!["a", "b"]);
        let meetings = result["meetings"].as_array().unwrap();
        assert_eq!(meetings.len(), 1);
        assert_eq!(meetings[0]["todo_id"].as_i64().unwrap(), parent_id);

        // verify persistence outside the txn — read fresh.
        let task_svc = build_sqlite_service_factory("task")(pool_ds.clone()).unwrap();
        let all_tasks = task_svc.invoke("findAll", json!({})).await.unwrap();
        assert_eq!(all_tasks.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn create_with_no_child_arrays_returns_empty_children() {
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        let base_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![direct_fk_binding("tasks", "task", "todo_id")],
        );
        let result = svc
            .invoke(
                "create",
                json!({ "body": { "title": "only", "is_done": false } }),
            )
            .await
            .unwrap();
        assert_eq!(result["tasks"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn create_singular_child_object_and_omitted_is_null() {
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        let mut binding = direct_fk_binding("meeting", "meeting", "todo_id");
        binding.binding.is_array = false;
        let base_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![binding],
        );
        let created = svc
            .invoke(
                "create",
                json!({
                    "body": {
                        "title": "demo",
                        "is_done": false,
                        "meeting": { "scheduled_at": "2026-01-01T00:00:00Z" }
                    }
                }),
            )
            .await
            .unwrap();
        assert!(created["meeting"].is_object(), "got {:?}", created["meeting"]);
        assert!(!created["meeting"].is_array());

        let mut omitted_binding = direct_fk_binding("meeting", "meeting", "todo_id");
        omitted_binding.binding.is_array = false;
        let omitted_base = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        let omitted_svc = EagerChildWritingService::new(
            omitted_base,
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![omitted_binding],
        );
        let omitted = omitted_svc
            .invoke(
                "create",
                json!({ "body": { "title": "empty", "is_done": false } }),
            )
            .await
            .unwrap();
        assert!(omitted["meeting"].is_null());
    }

    #[test]
    fn with_child_if_match_injects_version_when_child_is_tracked() {
        let child = json!({ "id": "c1", "updated": "2026-08-03T00:00:00Z" });
        let args = with_child_if_match(json!({ "id": "c1" }), &child);
        assert_eq!(args[IF_MATCH_ARG], json!("2026-08-03T00:00:00Z"));
    }

    #[test]
    fn with_child_if_match_leaves_untracked_child_untouched() {
        let child = json!({ "id": "c1" });
        let args = with_child_if_match(json!({ "id": "c1" }), &child);
        assert!(args.get(IF_MATCH_ARG).is_none());
    }

    #[tokio::test]
    async fn delete_removes_parent_and_children() {
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        let base_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![direct_fk_binding("tasks", "task", "todo_id")],
        );
        let created = svc
            .invoke(
                "create",
                json!({
                    "body": {
                        "title": "x",
                        "is_done": false,
                        "tasks": [{"description": "p"}, {"description": "q"}],
                    }
                }),
            )
            .await
            .unwrap();
        let pid = created["id"].clone();
        let del = svc.invoke("delete", json!({ "id": pid })).await.unwrap();
        assert_eq!(del["deleted"], json!(true));
        let task_svc = build_sqlite_service_factory("task")(pool_ds.clone()).unwrap();
        let remaining = task_svc.invoke("findAll", json!({})).await.unwrap();
        assert_eq!(remaining.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn update_reconciles_existing_children() {
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        let base_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![direct_fk_binding("tasks", "task", "todo_id")],
        );
        let created = svc
            .invoke(
                "create",
                json!({
                    "body": {
                        "title": "x",
                        "is_done": false,
                        "tasks": [{"description": "old1"}, {"description": "old2"}],
                    }
                }),
            )
            .await
            .unwrap();
        let pid = created["id"].clone();
        let original_first_id = created["tasks"][0]["id"].clone();

        let updated = svc
            .invoke(
                "update",
                json!({
                    "id": pid.clone(),
                    "body": {
                        "title": "x2",
                        "tasks": [
                            { "id": original_first_id, "description": "updated" },
                            { "description": "fresh" },
                        ],
                    }
                }),
            )
            .await
            .unwrap();
        assert_eq!(updated["title"], json!("x2"));
        let tasks = updated["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0]["description"], json!("updated"));
        assert_eq!(tasks[1]["description"], json!("fresh"));

        let task_svc = build_sqlite_service_factory("task")(pool_ds.clone()).unwrap();
        let all = task_svc.invoke("findAll", json!({})).await.unwrap();
        // old2 was missing from incoming → deleted, leaving 2 (updated + fresh).
        assert_eq!(all.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn patch_cascades_junction_deletes_when_replacing_direct_fk_child_that_has_m2m_grandchild(
    ) {
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();

        let shared_tag_repo = Arc::new(InMemoryCrudRepository::new());
        let tag_repo_for_factory = shared_tag_repo.clone();
        let tag_factory: ServiceFactory = Arc::new(move |_ds: Arc<dyn Datasource>| {
            let svc: Arc<dyn DynamicService> = Arc::new(GenericCrudService::new(
                tag_repo_for_factory.clone() as Arc<dyn CrudRepository>,
            ));
            Ok(svc)
        });
        let shared_task_tag_repo = Arc::new(InMemoryCrudRepository::new());
        let task_tag_repo_for_factory = shared_task_tag_repo.clone();
        let task_tag_junction_factory: CrudRepoFactory =
            Arc::new(move |_ds: Arc<dyn Datasource>| {
                Ok(task_tag_repo_for_factory.clone() as Arc<dyn CrudRepository>)
            });

        let tags_under_task = EagerWriteChildBindingRuntime {
            binding: EagerWriteChildBinding {
                kind: BindingKind::M2m {
                    junction_table: "task_tag".to_string(),
                    parent_fk_column: "task_id".to_string(),
                    target_fk_column: "tag_id".to_string(),
                },
                field_name: "tags".to_string(),
                child_table: "tag".to_string(),
                children: Vec::new(),
                is_array: true,
            },
            child_service_factory: tag_factory.clone(),
            junction_repo_factory: Some(task_tag_junction_factory.clone()),
            children: Vec::new(),
        };
        let tasks_under_todo = EagerWriteChildBindingRuntime {
            binding: EagerWriteChildBinding {
                kind: BindingKind::DirectFk {
                    fk_column: "todo_id".to_string(),
                },
                field_name: "tasks".to_string(),
                child_table: "task".to_string(),
                children: Vec::new(),
                is_array: true,
            },
            child_service_factory: build_sqlite_service_factory("task"),
            junction_repo_factory: None,
            children: vec![tags_under_task],
        };

        let base_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![tasks_under_todo],
        );

        let created = svc
            .invoke(
                "create",
                json!({
                    "body": {
                        "title": "trip",
                        "is_done": false,
                        "tasks": [
                            { "description": "pack", "tags": [{ "name": "urgent" }] },
                        ],
                    }
                }),
            )
            .await
            .unwrap();
        let pid = created["id"].clone();
        let original_task_id = created["tasks"][0]["id"].clone();

        assert_eq!(
            shared_task_tag_repo.find_all().await.unwrap().len(),
            1,
            "create should have inserted the task_tag junction row"
        );

        let _ = svc
            .invoke(
                "update",
                json!({
                    "id": pid.clone(),
                    "body": { "title": "trip", "tasks": [] }
                }),
            )
            .await
            .expect(
                "PATCH/update that removes a direct-fk child must cascade-delete the child's m2m junction rows (regression: in-memory repo doesn't trip FK 787 like sqlite, but the orphaned junction rows betray the same bug)",
            );

        let task_svc = build_sqlite_service_factory("task")(pool_ds.clone()).unwrap();
        let remaining_tasks = task_svc.invoke("findAll", json!({})).await.unwrap();
        assert_eq!(
            remaining_tasks.as_array().unwrap().len(),
            0,
            "the orphaned task should be deleted by the reconcile orphan-delete loop"
        );
        let leftover_junctions = shared_task_tag_repo.find_all().await.unwrap();
        assert_eq!(
            leftover_junctions.len(),
            0,
            "task_tag junction rows for the deleted task must be cascade-removed before the task itself is deleted; leftover rows = {} (task_id={:?})",
            leftover_junctions.len(),
            original_task_id,
        );
    }

    #[tokio::test]
    async fn rollback_on_child_insert_failure_leaves_no_parent() {
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        let base_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        // task table has no `bogus_col` — second insert will fail, rolling back parent.
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![direct_fk_binding("tasks", "task", "todo_id")],
        );
        let err = svc
            .invoke(
                "create",
                json!({
                    "body": {
                        "title": "doomed",
                        "is_done": false,
                        "tasks": [{ "bogus_col": "fails" }],
                    }
                }),
            )
            .await;
        assert!(err.is_err(), "expected failure, got {:?}", err);
        let parent_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        let parents = parent_svc.invoke("findAll", json!({})).await.unwrap();
        assert_eq!(parents.as_array().unwrap().len(), 0);
    }

    // M2M flow over the in-memory repo (no SQL needed) — verifies the m2m binding code path.
    fn build_inmem_service_factory() -> ServiceFactory {
        Arc::new(|_ds: Arc<dyn Datasource>| {
            let svc: Arc<dyn DynamicService> = Arc::new(GenericCrudService::new(Arc::new(
                InMemoryCrudRepository::new(),
            )));
            Ok(svc)
        })
    }

    #[tokio::test]
    async fn m2m_create_with_resolve_by_id_attaches_junction() {
        // pool datasource is unused by the in-mem factories — but we still need a real one for run_in_transaction.
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();

        // Shared target svc so the post-create read can see the same store.
        let shared_target_repo = Arc::new(InMemoryCrudRepository::new());
        let target_repo_for_factory = shared_target_repo.clone();
        let target_factory: ServiceFactory = Arc::new(move |_ds: Arc<dyn Datasource>| {
            let svc: Arc<dyn DynamicService> = Arc::new(GenericCrudService::new(
                target_repo_for_factory.clone() as Arc<dyn CrudRepository>,
            ));
            Ok(svc)
        });
        let shared_junction_repo = Arc::new(InMemoryCrudRepository::new());
        let junction_repo_for_factory = shared_junction_repo.clone();
        let junction_factory: CrudRepoFactory = Arc::new(move |_ds: Arc<dyn Datasource>| {
            Ok(junction_repo_for_factory.clone() as Arc<dyn CrudRepository>)
        });

        // Pre-seed two tags.
        let target_svc = target_factory(pool_ds.clone()).unwrap();
        let t1 = target_svc
            .invoke("create", json!({ "name": "rust" }))
            .await
            .unwrap();
        let _t2 = target_svc
            .invoke("create", json!({ "name": "axum" }))
            .await
            .unwrap();

        let parent_factory = build_inmem_service_factory();
        let base = parent_factory(pool_ds.clone()).unwrap();
        let bindings = vec![EagerWriteChildBindingRuntime {
            binding: EagerWriteChildBinding {
                kind: BindingKind::M2m {
                    junction_table: "post_tag".to_string(),
                    parent_fk_column: "post_id".to_string(),
                    target_fk_column: "tag_id".to_string(),
                },
                field_name: "tags".to_string(),
                child_table: "tag".to_string(),
                children: Vec::new(),
                is_array: true,
            },
            child_service_factory: target_factory.clone(),
            junction_repo_factory: Some(junction_factory.clone()),
            children: Vec::new(),
        }];
        let svc = EagerChildWritingService::new(
            base,
            pool_ds.clone(),
            parent_factory.clone(),
            "id",
            bindings,
        );

        let result = svc
            .invoke(
                "create",
                json!({
                    "body": {
                        "title": "hello",
                        "tags": [
                            { "id": t1["id"].clone() },
                            { "name": "new-tag" },
                        ],
                    }
                }),
            )
            .await
            .unwrap();
        let parent_id = result["id"].as_i64().unwrap();
        let tags = result["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        let names: Vec<&str> = tags.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["rust", "new-tag"]);

        // junction has two rows, both pointing at parent_id
        let junction_rows = shared_junction_repo.find_all().await.unwrap();
        assert_eq!(junction_rows.len(), 2);
        for r in &junction_rows {
            assert_eq!(r.get("post_id").and_then(|v| v.as_i64()), Some(parent_id));
        }
        // target tag table grew from 2 (seeded) to 3 (one new).
        let target_rows = shared_target_repo.find_all().await.unwrap();
        assert_eq!(target_rows.len(), 3);
    }

    #[tokio::test]
    async fn read_methods_delegate_to_base() {
        let ds = open_sqlite_with_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        // seed the parent table directly through a base svc.
        let parent_svc = build_sqlite_service_factory("todo")(pool_ds.clone()).unwrap();
        parent_svc
            .invoke("create", json!({ "title": "seeded", "is_done": false }))
            .await
            .unwrap();
        let svc = EagerChildWritingService::new(
            parent_svc.clone(),
            pool_ds.clone(),
            build_sqlite_service_factory("todo"),
            "id",
            vec![direct_fk_binding("tasks", "task", "todo_id")],
        );
        let rows = svc.invoke("findAll", json!({})).await.unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1);
    }

    // why this fixture exists: samples/email-backend POST /api/messages 500'd with "expected object, got string" because GenericCrudService::extract_data_row treated message.body (a real string column) as an envelope when the eager wrapper forwarded un-wrapped parent_data to the base svc.
    async fn open_sqlite_with_message_schema() -> Arc<SqliteDatasource> {
        let mut ds = SqliteDatasource::new(SqliteDatasourceOptions::in_memory());
        ds.open().await.unwrap();
        ds.query(
            "CREATE TABLE message (id INTEGER PRIMARY KEY AUTOINCREMENT, folder_id INTEGER, subject TEXT, body TEXT, sender TEXT)",
            &[],
        )
        .await
        .unwrap();
        ds.query(
            "CREATE TABLE attachment (id INTEGER PRIMARY KEY AUTOINCREMENT, message_id INTEGER, filename TEXT, data TEXT)",
            &[],
        )
        .await
        .unwrap();
        Arc::new(ds)
    }

    #[tokio::test]
    async fn create_message_persists_string_body_column_through_eager_wrapper() {
        let ds = open_sqlite_with_message_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        let base_svc = build_sqlite_service_factory("message")(pool_ds.clone()).unwrap();
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("message"),
            "id",
            vec![direct_fk_binding("attachments", "attachment", "message_id")],
        );
        // Shape mirrors dynamic.rs::build_args for POST /api/messages with {subject, body, sender, folder_id}: flattened top-level + an envelope `body` Object.
        let body_object = json!({
            "subject": "Welcome",
            "body": "Glad to have you on board.",
            "sender": "hr@example.com",
            "folder_id": 1,
        });
        let result = svc
            .invoke(
                "create",
                json!({
                    "subject": "Welcome",
                    "body": "Glad to have you on board.",
                    "sender": "hr@example.com",
                    "folder_id": 1,
                    // dynamic.rs adds this envelope key last, overwriting the flattened `body` string with the full request body Object.
                    "body": body_object,
                }),
            )
            .await
            .unwrap();
        assert_eq!(result["subject"], json!("Welcome"));
        assert_eq!(result["body"], json!("Glad to have you on board."));
        assert_eq!(result["sender"], json!("hr@example.com"));
        assert!(result["id"].is_number());
    }

    #[tokio::test]
    async fn create_direct_fk_message_with_data_envelope_keeps_body_string_column() {
        // Mirrors combined_routes::direct_fk_create which wraps the row in `{"data": Value::Object(data_map)}` before invoking create.
        let ds = open_sqlite_with_message_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        let base_svc = build_sqlite_service_factory("message")(pool_ds.clone()).unwrap();
        let svc = EagerChildWritingService::new(
            base_svc,
            pool_ds.clone(),
            build_sqlite_service_factory("message"),
            "id",
            vec![direct_fk_binding("attachments", "attachment", "message_id")],
        );
        let result = svc
            .invoke(
                "create",
                json!({
                    "data": {
                        "subject": "Re: Welcome",
                        "body": "Thanks for the warm welcome.",
                        "sender": "hr@example.com",
                        "folder_id": 1,
                    }
                }),
            )
            .await
            .unwrap();
        assert_eq!(result["body"], json!("Thanks for the warm welcome."));
        assert!(result["id"].is_number());
    }

    #[tokio::test]
    async fn create_attachment_with_string_data_column_persists() {
        let ds = open_sqlite_with_message_schema().await;
        let pool_ds: Arc<dyn Datasource> = ds.clone();
        // attachment service has no eager-write children; goes straight through GenericCrudService.
        let svc = build_sqlite_service_factory("attachment")(pool_ds.clone()).unwrap();
        let result = svc
            .invoke(
                "create",
                json!({
                    "data": {
                        "message_id": 1,
                        "filename": "handbook.pdf",
                        "data": "base64payloadhere",
                    }
                }),
            )
            .await
            .unwrap();
        assert_eq!(result["filename"], json!("handbook.pdf"));
        assert_eq!(result["data"], json!("base64payloadhere"));
        assert!(result["id"].is_number());
    }
}
