use std::collections::HashMap;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::{Path, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{on, MethodFilter};
use axum::{Json, Router};
use serde_json::{json, Map, Value};

use crate::error::RepositoryError;
use crate::loaders::{DatasourceTypeDef, FieldDef, RawCombinedChild, RawCombinedEntry};
use crate::router::dynamic::{coerce_path_param, extract_if_match};
use crate::router::ServiceRegistry;
use crate::services::{DynamicService, ServiceError, IF_MATCH_ARG};
use axum::http::HeaderMap;

use super::crud_routes::pluralize_path_segment;

const BODY_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum CombinedDescriptor {
    DirectFk(DirectFkDescriptor),
    M2m(M2mDescriptor),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirectFkDescriptor {
    pub parent: String,
    pub parent_base_path: String,
    pub parent_param: String,
    pub child: String,
    pub fk_column: String,
    pub collection_path: String,
    pub member_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct M2mDescriptor {
    pub parent: String,
    pub parent_base_path: String,
    pub parent_param: String,
    pub junction: String,
    pub target: String,
    pub target_param: String,
    pub parent_fk_field: String,
    pub child_fk_field: String,
    pub collection_path: String,
    pub member_path: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CombinedRoutesError {
    #[error("combined_routes '{parent}': child '{child}' missing both `via:` and `target:`")]
    M2mIncomplete { parent: String, child: String },
    #[error(
        "combined_routes '{parent}': junction '{junction}' not found in datasource_types.yaml"
    )]
    JunctionMissing { parent: String, junction: String },
    #[error("combined_routes '{parent}': child '{child}' not found in datasource_types.yaml")]
    ChildMissing { parent: String, child: String },
    #[error("combined_routes '{parent}': child '{child}' has no FK to parent and no junction in datasource_types.yaml")]
    NoRelationship { parent: String, child: String },
    #[error("combined_routes '{parent}': ambiguous junction between '{parent}' and '{child}' — candidates: {candidates}. Add via: to disambiguate.")]
    AmbiguousJunction {
        parent: String,
        child: String,
        candidates: String,
    },
    #[error("combined_routes '{parent}': junction '{junction}' missing FK to '{side}'")]
    JunctionMissingFk {
        parent: String,
        junction: String,
        side: String,
    },
}

pub fn expand_combined_routes(
    entries: &[RawCombinedEntry],
    entities: &[DatasourceTypeDef],
) -> Result<Vec<CombinedDescriptor>, CombinedRoutesError> {
    let by_name: HashMap<&str, &DatasourceTypeDef> =
        entities.iter().map(|e| (e.name.as_str(), e)).collect();
    let mut out = Vec::new();
    for entry in entries {
        let parent_param = parent_param_name(&entry.parent);
        let parent_base = rewrite_parent_path(&entry.route, &parent_param);
        for child in &entry.children {
            out.push(expand_child(
                entry,
                child,
                &parent_base,
                &parent_param,
                &by_name,
            )?);
        }
    }
    Ok(out)
}

fn expand_child(
    entry: &RawCombinedEntry,
    child: &RawCombinedChild,
    parent_base: &str,
    parent_param: &str,
    by_name: &HashMap<&str, &DatasourceTypeDef>,
) -> Result<CombinedDescriptor, CombinedRoutesError> {
    if child.via.is_some() || child.target.is_some() {
        let junction = child
            .via
            .as_deref()
            .ok_or_else(|| CombinedRoutesError::M2mIncomplete {
                parent: entry.parent.clone(),
                child: child.name.clone(),
            })?;
        let target = child
            .target
            .as_deref()
            .ok_or_else(|| CombinedRoutesError::M2mIncomplete {
                parent: entry.parent.clone(),
                child: child.name.clone(),
            })?;
        let junction_def =
            by_name
                .get(junction)
                .ok_or_else(|| CombinedRoutesError::JunctionMissing {
                    parent: entry.parent.clone(),
                    junction: junction.to_string(),
                })?;
        let parent_fk = find_foreign_key_to(junction_def, &entry.parent).ok_or_else(|| {
            CombinedRoutesError::JunctionMissingFk {
                parent: entry.parent.clone(),
                junction: junction.to_string(),
                side: entry.parent.clone(),
            }
        })?;
        let child_fk = find_foreign_key_to(junction_def, target).ok_or_else(|| {
            CombinedRoutesError::JunctionMissingFk {
                parent: entry.parent.clone(),
                junction: junction.to_string(),
                side: target.to_string(),
            }
        })?;
        let segment = child
            .route
            .clone()
            .unwrap_or_else(|| default_segment(target));
        let collection = format!("{}{}", parent_base, segment);
        let target_param = format!("{}Id", snake_to_camel(target));
        let member = format!("{}/{{{}}}", collection, target_param);
        return Ok(CombinedDescriptor::M2m(M2mDescriptor {
            parent: entry.parent.clone(),
            parent_base_path: parent_base.to_string(),
            parent_param: parent_param.to_string(),
            junction: junction.to_string(),
            target: target.to_string(),
            target_param,
            parent_fk_field: parent_fk,
            child_fk_field: child_fk,
            collection_path: collection,
            member_path: member,
        }));
    }

    let child_def =
        by_name
            .get(child.name.as_str())
            .ok_or_else(|| CombinedRoutesError::ChildMissing {
                parent: entry.parent.clone(),
                child: child.name.clone(),
            })?;
    if let Some(fk) = find_foreign_key_to(child_def, &entry.parent) {
        let segment = child
            .route
            .clone()
            .unwrap_or_else(|| default_segment(&child.name));
        let collection = format!("{}{}", parent_base, segment);
        let member = format!("{}/{{id}}", collection);
        return Ok(CombinedDescriptor::DirectFk(DirectFkDescriptor {
            parent: entry.parent.clone(),
            parent_base_path: parent_base.to_string(),
            parent_param: parent_param.to_string(),
            child: child.name.clone(),
            fk_column: fk,
            collection_path: collection,
            member_path: member,
        }));
    }

    let mut candidates = Vec::new();
    for (name, def) in by_name {
        if *name == entry.parent || *name == child.name {
            continue;
        }
        let parent_fk = find_foreign_key_to(def, &entry.parent);
        let child_fk = find_foreign_key_to(def, &child.name);
        if parent_fk.is_some() && child_fk.is_some() {
            candidates.push((name.to_string(), parent_fk.unwrap(), child_fk.unwrap()));
        }
    }
    if candidates.is_empty() {
        return Err(CombinedRoutesError::NoRelationship {
            parent: entry.parent.clone(),
            child: child.name.clone(),
        });
    }
    if candidates.len() > 1 {
        let names: Vec<String> = candidates.into_iter().map(|c| c.0).collect();
        return Err(CombinedRoutesError::AmbiguousJunction {
            parent: entry.parent.clone(),
            child: child.name.clone(),
            candidates: names.join(", "),
        });
    }
    let (junction_name, parent_fk, child_fk) = candidates.into_iter().next().unwrap();
    let segment = child
        .route
        .clone()
        .unwrap_or_else(|| default_segment(&child.name));
    let collection = format!("{}{}", parent_base, segment);
    let target_param = format!("{}Id", snake_to_camel(&child.name));
    let member = format!("{}/{{{}}}", collection, target_param);
    Ok(CombinedDescriptor::M2m(M2mDescriptor {
        parent: entry.parent.clone(),
        parent_base_path: parent_base.to_string(),
        parent_param: parent_param.to_string(),
        junction: junction_name,
        target: child.name.clone(),
        target_param,
        parent_fk_field: parent_fk,
        child_fk_field: child_fk,
        collection_path: collection,
        member_path: member,
    }))
}

fn default_segment(name: &str) -> String {
    format!("/{}", pluralize_path_segment(name))
}

fn parent_param_name(parent: &str) -> String {
    format!("{}Id", snake_to_camel(parent))
}

fn rewrite_parent_path(raw: &str, parent_param: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let close = match raw[i..].find('}') {
                Some(off) => i + off,
                None => {
                    out.push('{');
                    i += 1;
                    continue;
                }
            };
            let name = &raw[i + 1..close];
            if name == "id" {
                out.push('{');
                out.push_str(parent_param);
                out.push('}');
            } else {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
            i = close + 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

pub fn find_foreign_key_to(def: &DatasourceTypeDef, parent: &str) -> Option<String> {
    for f in &def.fields {
        if fk_target(f) == Some(parent) {
            return Some(f.name.clone());
        }
    }
    None
}

fn fk_target(f: &FieldDef) -> Option<&str> {
    let r = f.references.as_deref()?;
    Some(r.split('.').next().unwrap_or(r))
}

fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper = false;
    for c in s.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            for cc in c.to_uppercase() {
                out.push(cc);
            }
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn mount_combined_routes(
    router: Router,
    descriptors: &[CombinedDescriptor],
    registry: &ServiceRegistry,
    raw_registry: &ServiceRegistry,
) -> Router {
    let mut router = router;
    for d in descriptors {
        match d {
            CombinedDescriptor::DirectFk(desc) => {
                router = mount_direct_fk(router, desc, registry);
            }
            CombinedDescriptor::M2m(desc) => {
                router = mount_m2m(router, desc, registry, raw_registry);
            }
        }
    }
    router
}

fn mount_direct_fk(
    router: Router,
    desc: &DirectFkDescriptor,
    registry: &ServiceRegistry,
) -> Router {
    let Some(service) = registry.get(&desc.child) else {
        return router;
    };
    let collection = desc.collection_path.clone();
    let member = desc.member_path.clone();
    let parent_param = desc.parent_param.clone();
    let fk = desc.fk_column.clone();
    let entity_name = pascal(&desc.child);

    let svc = service.clone();
    let pp = parent_param.clone();
    let fk_c = fk.clone();
    let en = entity_name.clone();
    let list_handler = move |Path(params): Path<HashMap<String, String>>| {
        let svc = svc.clone();
        let pp = pp.clone();
        let fk_c = fk_c.clone();
        let en = en.clone();
        async move { direct_fk_list(svc, params, pp, fk_c, en).await }
    };

    let svc = service.clone();
    let pp = parent_param.clone();
    let fk_c = fk.clone();
    let en = entity_name.clone();
    let get_handler = move |Path(params): Path<HashMap<String, String>>| {
        let svc = svc.clone();
        let pp = pp.clone();
        let fk_c = fk_c.clone();
        let en = en.clone();
        async move { direct_fk_get(svc, params, pp, fk_c, en).await }
    };

    let svc = service.clone();
    let pp = parent_param.clone();
    let fk_c = fk.clone();
    let en = entity_name.clone();
    let post_handler = move |Path(params): Path<HashMap<String, String>>, req: Request| {
        let svc = svc.clone();
        let pp = pp.clone();
        let fk_c = fk_c.clone();
        let en = en.clone();
        async move { direct_fk_create(svc, params, req, pp, fk_c, en).await }
    };

    let svc = service.clone();
    let pp = parent_param.clone();
    let fk_c = fk.clone();
    let en = entity_name.clone();
    let put_handler = move |Path(params): Path<HashMap<String, String>>, req: Request| {
        let svc = svc.clone();
        let pp = pp.clone();
        let fk_c = fk_c.clone();
        let en = en.clone();
        async move { direct_fk_update(svc, params, req, pp, fk_c, en).await }
    };

    let svc = service.clone();
    let pp = parent_param.clone();
    let fk_c = fk.clone();
    let en = entity_name.clone();
    let patch_handler = move |Path(params): Path<HashMap<String, String>>, req: Request| {
        let svc = svc.clone();
        let pp = pp.clone();
        let fk_c = fk_c.clone();
        let en = en.clone();
        async move { direct_fk_update(svc, params, req, pp, fk_c, en).await }
    };

    let svc = service.clone();
    let pp = parent_param.clone();
    let fk_c = fk.clone();
    let en = entity_name.clone();
    let delete_handler = move |Path(params): Path<HashMap<String, String>>, headers: HeaderMap| {
        let svc = svc.clone();
        let pp = pp.clone();
        let fk_c = fk_c.clone();
        let en = en.clone();
        async move { direct_fk_delete(svc, params, headers, pp, fk_c, en).await }
    };

    router
        .route(&collection, on(MethodFilter::GET, list_handler))
        .route(&collection, on(MethodFilter::POST, post_handler))
        .route(&member, on(MethodFilter::GET, get_handler))
        .route(&member, on(MethodFilter::PUT, put_handler))
        .route(&member, on(MethodFilter::PATCH, patch_handler))
        .route(&member, on(MethodFilter::DELETE, delete_handler))
}

async fn direct_fk_list(
    svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    parent_param: String,
    fk_column: String,
    _entity_name: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let rows = match svc.invoke("findAll", json!({})).await {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    let arr = rows.as_array().cloned().unwrap_or_default();
    let filtered: Vec<Value> = arr
        .into_iter()
        .filter(|row| matches_parent_id(row, &fk_column, &parent_id))
        .collect();
    (StatusCode::OK, Json(json!({ "items": filtered }))).into_response()
}

async fn direct_fk_get(
    svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    parent_param: String,
    fk_column: String,
    entity_name: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let id = match id_param(&params, "id") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let item = match svc.invoke("findById", json!({ "id": &id })).await {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    if item.is_null() {
        return not_found_response(&entity_name, &id);
    }
    if !matches_parent_id(&item, &fk_column, &parent_id) {
        return not_found_response(&entity_name, &id);
    }
    (StatusCode::OK, Json(item)).into_response()
}

async fn direct_fk_create(
    svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    req: Request,
    parent_param: String,
    fk_column: String,
    _entity_name: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let body = match read_json_body(req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let mut data_map = body_as_map(body);
    data_map.insert(fk_column.clone(), json!(&parent_id));
    let created = match svc
        .invoke("create", json!({ "data": Value::Object(data_map) }))
        .await
    {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    (StatusCode::CREATED, Json(created)).into_response()
}

async fn direct_fk_update(
    svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    req: Request,
    parent_param: String,
    fk_column: String,
    entity_name: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let id = match id_param(&params, "id") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let if_match = extract_if_match(req.headers());
    let existing = match svc.invoke("findById", json!({ "id": &id })).await {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    if existing.is_null() || !matches_parent_id(&existing, &fk_column, &parent_id) {
        return not_found_response(&entity_name, &id);
    }
    let body = match read_json_body(req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let mut data_map = body_as_map(body);
    data_map.insert(fk_column.clone(), json!(&parent_id));
    let mut update_args = json!({ "id": id, "data": Value::Object(data_map) });
    if let (Some(m), Value::Object(map)) = (&if_match, &mut update_args) {
        map.insert(IF_MATCH_ARG.to_string(), Value::String(m.clone()));
    }
    let updated = match svc.invoke("update", update_args).await {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    if updated.is_null() {
        return not_found_response(&entity_name, &id);
    }
    (StatusCode::OK, Json(updated)).into_response()
}

async fn direct_fk_delete(
    svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    headers: HeaderMap,
    parent_param: String,
    fk_column: String,
    entity_name: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let id = match id_param(&params, "id") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let if_match = extract_if_match(&headers);
    let existing = match svc.invoke("findById", json!({ "id": &id })).await {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    if existing.is_null() || !matches_parent_id(&existing, &fk_column, &parent_id) {
        return not_found_response(&entity_name, &id);
    }
    let mut delete_args = json!({ "id": &id });
    if let (Some(m), Value::Object(map)) = (&if_match, &mut delete_args) {
        map.insert(IF_MATCH_ARG.to_string(), Value::String(m.clone()));
    }
    if let Err(e) = svc.invoke("delete", delete_args).await {
        return service_error_response(e);
    }
    (StatusCode::OK, Json(json!({ "success": true }))).into_response()
}

fn mount_m2m(
    router: Router,
    desc: &M2mDescriptor,
    registry: &ServiceRegistry,
    raw_registry: &ServiceRegistry,
) -> Router {
    let Some(parent_svc) = registry.get(&desc.parent) else {
        return router;
    };
    // Raw (pre-enrichment) junction service: the handlers filter junction rows by the raw parent/child
    // FK columns, which enrichment strips on the registry service — so the join reads must be unenriched.
    let Some(junction_svc) = raw_registry.get(&desc.junction) else {
        return router;
    };
    let Some(target_svc) = registry.get(&desc.target) else {
        return router;
    };

    let collection = desc.collection_path.clone();
    let member = desc.member_path.clone();
    let parent_param = desc.parent_param.clone();
    let target_param = desc.target_param.clone();
    let parent_fk = desc.parent_fk_field.clone();
    let child_fk = desc.child_fk_field.clone();
    let parent_entity = pascal(&desc.parent);
    let target_entity = pascal(&desc.target);

    let ps = parent_svc.clone();
    let js = junction_svc.clone();
    let ts = target_svc.clone();
    let pp = parent_param.clone();
    let pfk = parent_fk.clone();
    let cfk = child_fk.clone();
    let pe = parent_entity.clone();
    let list = move |Path(params): Path<HashMap<String, String>>| {
        let ps = ps.clone();
        let js = js.clone();
        let ts = ts.clone();
        let pp = pp.clone();
        let pfk = pfk.clone();
        let cfk = cfk.clone();
        let pe = pe.clone();
        async move { m2m_list(ps, js, ts, params, pp, pfk, cfk, pe).await }
    };

    let ps = parent_svc.clone();
    let js = junction_svc.clone();
    let ts = target_svc.clone();
    let pp = parent_param.clone();
    let pfk = parent_fk.clone();
    let cfk = child_fk.clone();
    let pe = parent_entity.clone();
    let te = target_entity.clone();
    let create = move |Path(params): Path<HashMap<String, String>>, req: Request| {
        let ps = ps.clone();
        let js = js.clone();
        let ts = ts.clone();
        let pp = pp.clone();
        let pfk = pfk.clone();
        let cfk = cfk.clone();
        let pe = pe.clone();
        let te = te.clone();
        async move { m2m_create(ps, js, ts, params, req, pp, pfk, cfk, pe, te).await }
    };

    let ps = parent_svc.clone();
    let js = junction_svc.clone();
    let ts = target_svc.clone();
    let pp = parent_param.clone();
    let tp = target_param.clone();
    let pfk = parent_fk.clone();
    let cfk = child_fk.clone();
    let pe = parent_entity.clone();
    let te = target_entity.clone();
    let member_get = move |Path(params): Path<HashMap<String, String>>| {
        let ps = ps.clone();
        let js = js.clone();
        let ts = ts.clone();
        let pp = pp.clone();
        let tp = tp.clone();
        let pfk = pfk.clone();
        let cfk = cfk.clone();
        let pe = pe.clone();
        let te = te.clone();
        async move { m2m_get(ps, js, ts, params, pp, tp, pfk, cfk, pe, te).await }
    };

    let ps = parent_svc.clone();
    let js = junction_svc.clone();
    let ts = target_svc.clone();
    let pp = parent_param.clone();
    let tp = target_param.clone();
    let pfk = parent_fk.clone();
    let cfk = child_fk.clone();
    let pe = parent_entity.clone();
    let te = target_entity.clone();
    let member_link = move |Path(params): Path<HashMap<String, String>>| {
        let ps = ps.clone();
        let js = js.clone();
        let ts = ts.clone();
        let pp = pp.clone();
        let tp = tp.clone();
        let pfk = pfk.clone();
        let cfk = cfk.clone();
        let pe = pe.clone();
        let te = te.clone();
        async move { m2m_link(ps, js, ts, params, pp, tp, pfk, cfk, pe, te).await }
    };

    let ps = parent_svc.clone();
    let js = junction_svc.clone();
    let pp = parent_param.clone();
    let tp = target_param.clone();
    let pfk = parent_fk.clone();
    let cfk = child_fk.clone();
    let pe = parent_entity.clone();
    let te = target_entity.clone();
    let member_delete = move |Path(params): Path<HashMap<String, String>>| {
        let ps = ps.clone();
        let js = js.clone();
        let pp = pp.clone();
        let tp = tp.clone();
        let pfk = pfk.clone();
        let cfk = cfk.clone();
        let pe = pe.clone();
        let te = te.clone();
        async move { m2m_delete(ps, js, params, pp, tp, pfk, cfk, pe, te).await }
    };

    router
        .route(&collection, on(MethodFilter::GET, list))
        .route(&collection, on(MethodFilter::POST, create))
        .route(&member, on(MethodFilter::GET, member_get))
        .route(&member, on(MethodFilter::POST, member_link))
        .route(&member, on(MethodFilter::DELETE, member_delete))
}

#[allow(clippy::too_many_arguments)]
async fn m2m_list(
    parent_svc: Arc<dyn DynamicService>,
    junction_svc: Arc<dyn DynamicService>,
    target_svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    parent_param: String,
    parent_fk: String,
    child_fk: String,
    parent_entity: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !exists_by_id(&parent_svc, &parent_id).await.unwrap_or(false) {
        return not_found_response(&parent_entity, &parent_id);
    }
    let junctions = match junction_svc.invoke("findAll", json!({})).await {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    let arr = junctions.as_array().cloned().unwrap_or_default();
    let matching: Vec<Value> = arr
        .into_iter()
        .filter(|j| matches_parent_id(j, &parent_fk, &parent_id))
        .collect();
    if matching.is_empty() {
        return (StatusCode::OK, Json(json!({ "items": [] }))).into_response();
    }
    let targets = match target_svc.invoke("findAll", json!({})).await {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    let target_index = index_by_id(&targets);
    let resolved: Vec<Value> = matching
        .iter()
        .filter_map(|j| {
            let cid = j.get(&child_fk).map(id_key)?;
            target_index.get(&cid).cloned()
        })
        .collect();
    (StatusCode::OK, Json(json!({ "items": resolved }))).into_response()
}

#[allow(clippy::too_many_arguments)]
async fn m2m_create(
    parent_svc: Arc<dyn DynamicService>,
    junction_svc: Arc<dyn DynamicService>,
    target_svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    req: Request,
    parent_param: String,
    parent_fk: String,
    child_fk: String,
    parent_entity: String,
    target_entity: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !exists_by_id(&parent_svc, &parent_id).await.unwrap_or(false) {
        return not_found_response(&parent_entity, &parent_id);
    }
    let body = match read_json_body(req).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let body_map = body_as_map(body);
    let child_id = match id_from_body(body_map.get(&child_fk)) {
        Some(v) => v,
        None => return validation_error(&format!("{}: must be a valid id", child_fk)),
    };
    match target_svc
        .invoke("findById", json!({ "id": &child_id }))
        .await
    {
        Ok(v) if !v.is_null() => {}
        Ok(_) => return not_found_response(&target_entity, &child_id),
        Err(e) => return service_error_response(e),
    }
    let mut data = Map::new();
    data.insert(parent_fk.clone(), json!(&parent_id));
    data.insert(child_fk.clone(), json!(&child_id));
    if let Err(e) = junction_svc
        .invoke("create", json!({ "data": Value::Object(data) }))
        .await
    {
        return service_error_response(e);
    }
    let child = match target_svc
        .invoke("findById", json!({ "id": &child_id }))
        .await
    {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    (StatusCode::CREATED, Json(child)).into_response()
}

#[allow(clippy::too_many_arguments)]
async fn m2m_get(
    parent_svc: Arc<dyn DynamicService>,
    junction_svc: Arc<dyn DynamicService>,
    target_svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    parent_param: String,
    target_param: String,
    parent_fk: String,
    child_fk: String,
    parent_entity: String,
    target_entity: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match id_param(&params, &target_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !exists_by_id(&parent_svc, &parent_id).await.unwrap_or(false) {
        return not_found_response(&parent_entity, &parent_id);
    }
    if !junction_exists(&junction_svc, &parent_fk, &parent_id, &child_fk, &child_id).await {
        return junction_member_not_found(&target_entity, &child_id, &parent_entity, &parent_id);
    }
    let child = match target_svc
        .invoke("findById", json!({ "id": &child_id }))
        .await
    {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    if child.is_null() {
        return not_found_response(&target_entity, &child_id);
    }
    (StatusCode::OK, Json(child)).into_response()
}

#[allow(clippy::too_many_arguments)]
async fn m2m_link(
    parent_svc: Arc<dyn DynamicService>,
    junction_svc: Arc<dyn DynamicService>,
    target_svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    parent_param: String,
    target_param: String,
    parent_fk: String,
    child_fk: String,
    parent_entity: String,
    target_entity: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match id_param(&params, &target_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !exists_by_id(&parent_svc, &parent_id).await.unwrap_or(false) {
        return not_found_response(&parent_entity, &parent_id);
    }
    let child = match target_svc
        .invoke("findById", json!({ "id": &child_id }))
        .await
    {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    if child.is_null() {
        return not_found_response(&target_entity, &child_id);
    }
    if !junction_exists(&junction_svc, &parent_fk, &parent_id, &child_fk, &child_id).await {
        let mut data = Map::new();
        data.insert(parent_fk.clone(), json!(parent_id));
        data.insert(child_fk.clone(), json!(child_id));
        if let Err(e) = junction_svc
            .invoke("create", json!({ "data": Value::Object(data) }))
            .await
        {
            return service_error_response(e);
        }
    }
    (StatusCode::CREATED, Json(child)).into_response()
}

#[allow(clippy::too_many_arguments)]
async fn m2m_delete(
    parent_svc: Arc<dyn DynamicService>,
    junction_svc: Arc<dyn DynamicService>,
    params: HashMap<String, String>,
    parent_param: String,
    target_param: String,
    parent_fk: String,
    child_fk: String,
    parent_entity: String,
    target_entity: String,
) -> Response {
    let parent_id = match id_param(&params, &parent_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let child_id = match id_param(&params, &target_param) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if !exists_by_id(&parent_svc, &parent_id).await.unwrap_or(false) {
        return not_found_response(&parent_entity, &parent_id);
    }
    let junctions = match junction_svc.invoke("findAll", json!({})).await {
        Ok(v) => v,
        Err(e) => return service_error_response(e),
    };
    let arr = junctions.as_array().cloned().unwrap_or_default();
    let mapping = arr.iter().find(|j| {
        matches_parent_id(j, &parent_fk, &parent_id) && matches_parent_id(j, &child_fk, &child_id)
    });
    let Some(m) = mapping else {
        return junction_member_not_found(&target_entity, &child_id, &parent_entity, &parent_id);
    };
    let Some(jid) = m.get("id").cloned() else {
        return service_error_response(ServiceError::InvalidArgs(
            "junction row missing id".to_string(),
        ));
    };
    if let Err(e) = junction_svc.invoke("delete", json!({ "id": jid })).await {
        return service_error_response(e);
    }
    (StatusCode::OK, Json(json!({ "success": true }))).into_response()
}

/// A required path-param id, coerced to an opaque JSON value the same way the main CRUD router
/// coerces `{id}` (numeric-looking → Number so integer PK columns match, otherwise a String for
/// uuid / string keys). Missing or blank → 400.
fn id_param(params: &HashMap<String, String>, name: &str) -> Result<Value, Response> {
    match params.get(name).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(raw) => Ok(coerce_path_param(raw)),
        None => Err(validation_error(&format!("{}: must be provided", name))),
    }
}

/// The canonical string form of an id value, so a foreign-key column stored as a number or a
/// string across dialects compares equal to a coerced path-param id regardless of representation.
fn id_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The child id carried in an m2m create body: a positive number or a non-empty string.
fn id_from_body(v: Option<&Value>) -> Option<Value> {
    match v? {
        Value::Number(n) if n.as_i64().map(|i| i > 0).unwrap_or(true) => {
            Some(Value::Number(n.clone()))
        }
        Value::String(s) if !s.trim().is_empty() => Some(Value::String(s.clone())),
        _ => None,
    }
}

async fn read_json_body(req: Request) -> Result<Value, Response> {
    let (_, body) = req.into_parts();
    let bytes = match to_bytes(body, BODY_LIMIT).await {
        Ok(b) => b,
        Err(e) => {
            return Err(
                (StatusCode::BAD_REQUEST, format!("body read failed: {}", e)).into_response(),
            )
        }
    };
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(&bytes)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid JSON body: {}", e)).into_response())
}

fn body_as_map(body: Value) -> Map<String, Value> {
    match body {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn matches_parent_id(row: &Value, fk_column: &str, parent_id: &Value) -> bool {
    match row.get(fk_column) {
        Some(v) => id_key(v) == id_key(parent_id),
        None => false,
    }
}

fn index_by_id(rows: &Value) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    if let Some(arr) = rows.as_array() {
        for row in arr {
            if let Some(id) = row.get("id") {
                out.insert(id_key(id), row.clone());
            }
        }
    }
    out
}

async fn exists_by_id(svc: &Arc<dyn DynamicService>, id: &Value) -> Result<bool, ServiceError> {
    let v = svc.invoke("findById", json!({ "id": id })).await?;
    Ok(!v.is_null())
}

async fn junction_exists(
    junction_svc: &Arc<dyn DynamicService>,
    parent_fk: &str,
    parent_id: &Value,
    child_fk: &str,
    child_id: &Value,
) -> bool {
    let Ok(rows) = junction_svc.invoke("findAll", json!({})).await else {
        return false;
    };
    let Some(arr) = rows.as_array() else {
        return false;
    };
    arr.iter().any(|j| {
        matches_parent_id(j, parent_fk, parent_id) && matches_parent_id(j, child_fk, child_id)
    })
}

fn not_found_response(entity_name: &str, id: &Value) -> Response {
    let body = json!({
        "errors": [{
            "code": "NOT_FOUND",
            "message": format!("{} with id '{}' not found", entity_name, id_key(id)),
        }],
    });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn junction_member_not_found(
    target_entity: &str,
    child_id: &Value,
    parent_entity: &str,
    parent_id: &Value,
) -> Response {
    let body = json!({
        "errors": [{
            "code": "NOT_FOUND",
            "message": format!(
                "{} '{}' not found for {} '{}'",
                target_entity,
                id_key(child_id),
                parent_entity,
                id_key(parent_id)
            ),
        }],
    });
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn validation_error(msg: &str) -> Response {
    let body = json!({
        "errors": [{
            "code": "VALIDATION_ERROR",
            "message": msg,
        }],
    });
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

fn service_error_response(err: ServiceError) -> Response {
    if let ServiceError::Stub(body) = err {
        return (StatusCode::NOT_IMPLEMENTED, Json(body)).into_response();
    }
    let (status, code) = match &err {
        ServiceError::UnknownMethod(_) => (StatusCode::METHOD_NOT_ALLOWED, "METHOD_NOT_ALLOWED"),
        ServiceError::InvalidArgs(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
        ServiceError::PreconditionRequired(_) => {
            (StatusCode::PRECONDITION_REQUIRED, "PRECONDITION_REQUIRED")
        }
        ServiceError::Repository(RepositoryError::ConcurrencyConflict(_)) => {
            (StatusCode::PRECONDITION_FAILED, "PRECONDITION_FAILED")
        }
        ServiceError::Repository(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        ServiceError::Stub(_) => unreachable!("handled above"),
    };
    let body = json!({
        "errors": [{
            "code": code,
            "message": err.to_string(),
        }],
    });
    (status, Json(body)).into_response()
}

fn pascal(snake: &str) -> String {
    let camel = snake_to_camel(snake);
    let mut chars = camel.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::FieldDef;

    #[test]
    fn id_key_normalizes_number_and_string_alike() {
        assert_eq!(id_key(&json!(5)), "5");
        assert_eq!(id_key(&json!("5")), "5");
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(id_key(&json!(uuid)), uuid);
    }

    #[test]
    fn matches_parent_id_compares_across_representations() {
        let row_uuid = json!({ "conversation_id": "abc-uuid" });
        assert!(matches_parent_id(
            &row_uuid,
            "conversation_id",
            &json!("abc-uuid")
        ));
        assert!(!matches_parent_id(
            &row_uuid,
            "conversation_id",
            &json!("other")
        ));
        let row_int = json!({ "conversation_id": 7 });
        assert!(matches_parent_id(&row_int, "conversation_id", &json!(7)));
        let row_str = json!({ "conversation_id": "7" });
        assert!(matches_parent_id(&row_str, "conversation_id", &json!(7)));
        assert!(!matches_parent_id(&row_int, "missing_col", &json!(7)));
    }

    #[test]
    fn id_param_coerces_uuid_to_string_and_int_to_number() {
        let mut params = HashMap::new();
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        params.insert("conversationId".to_string(), uuid.to_string());
        params.insert("id".to_string(), "42".to_string());
        assert_eq!(id_param(&params, "conversationId").unwrap(), json!(uuid));
        assert_eq!(id_param(&params, "id").unwrap(), json!(42));
        assert!(id_param(&params, "missing").is_err());
    }

    #[test]
    fn id_from_body_accepts_uuid_string_and_positive_int_only() {
        assert_eq!(id_from_body(Some(&json!("uuid-x"))), Some(json!("uuid-x")));
        assert_eq!(id_from_body(Some(&json!(3))), Some(json!(3)));
        assert_eq!(id_from_body(Some(&json!(""))), None);
        assert_eq!(id_from_body(Some(&json!(0))), None);
        assert_eq!(id_from_body(Some(&json!(null))), None);
        assert_eq!(id_from_body(None), None);
    }

    fn make_entity(name: &str, fks: &[(&str, &str)]) -> DatasourceTypeDef {
        let mut fields = vec![FieldDef {
            name: "id".to_string(),
            r#type: "integer".to_string(),
            primary_key: true,
            ..Default::default()
        }];
        for (name, refs) in fks {
            fields.push(FieldDef {
                name: name.to_string(),
                r#type: "integer".to_string(),
                references: Some(refs.to_string()),
                ..Default::default()
            });
        }
        DatasourceTypeDef {
            name: name.to_string(),
            fields,
            ..Default::default()
        }
    }

    #[test]
    fn snake_to_camel_basic() {
        assert_eq!(snake_to_camel("contact"), "contact");
        assert_eq!(snake_to_camel("contact_group"), "contactGroup");
        assert_eq!(snake_to_camel("contact_group_member"), "contactGroupMember");
    }

    #[test]
    fn rewrite_parent_path_replaces_id_with_camel_param() {
        let p = rewrite_parent_path("/api/contacts/{id}", "contactId");
        assert_eq!(p, "/api/contacts/{contactId}");
    }

    #[test]
    fn rewrite_parent_path_preserves_other_placeholders() {
        let p = rewrite_parent_path("/api/orgs/{slug}/projects/{id}", "projectId");
        assert_eq!(p, "/api/orgs/{slug}/projects/{projectId}");
    }

    #[test]
    fn expand_direct_fk_uses_explicit_route_segment() {
        let entry = RawCombinedEntry {
            parent: "contact".to_string(),
            route: "/api/contacts/{id}".to_string(),
            children: vec![RawCombinedChild {
                name: "address".to_string(),
                via: None,
                target: None,
                route: Some("/addresses".to_string()),
            }],
        };
        let contact = make_entity("contact", &[]);
        let address = make_entity("address", &[("contact_id", "contact.id")]);
        let out = expand_combined_routes(&[entry], &[contact, address]).unwrap();
        assert_eq!(out.len(), 1);
        let CombinedDescriptor::DirectFk(d) = &out[0] else {
            panic!("expected DirectFk")
        };
        assert_eq!(d.parent_param, "contactId");
        assert_eq!(d.fk_column, "contact_id");
        assert_eq!(d.collection_path, "/api/contacts/{contactId}/addresses");
        assert_eq!(d.member_path, "/api/contacts/{contactId}/addresses/{id}");
    }

    #[test]
    fn expand_direct_fk_falls_back_to_default_segment() {
        let entry = RawCombinedEntry {
            parent: "contact".to_string(),
            route: "/api/contacts/{id}".to_string(),
            children: vec![RawCombinedChild {
                name: "phone".to_string(),
                via: None,
                target: None,
                route: None,
            }],
        };
        let contact = make_entity("contact", &[]);
        let phone = make_entity("phone", &[("contact_id", "contact.id")]);
        let out = expand_combined_routes(&[entry], &[contact, phone]).unwrap();
        let CombinedDescriptor::DirectFk(d) = &out[0] else {
            panic!("expected DirectFk")
        };
        assert_eq!(d.collection_path, "/api/contacts/{contactId}/phones");
    }

    #[test]
    fn expand_m2m_via_target() {
        let entry = RawCombinedEntry {
            parent: "contact_group".to_string(),
            route: "/api/contact-groups/{id}".to_string(),
            children: vec![RawCombinedChild {
                name: "contact".to_string(),
                via: Some("contact_group_member".to_string()),
                target: Some("contact".to_string()),
                route: Some("/members".to_string()),
            }],
        };
        let contact = make_entity("contact", &[]);
        let contact_group = make_entity("contact_group", &[]);
        let junction = make_entity(
            "contact_group_member",
            &[
                ("contact_id", "contact.id"),
                ("contact_group_id", "contact_group.id"),
            ],
        );
        let out = expand_combined_routes(&[entry], &[contact, contact_group, junction]).unwrap();
        let CombinedDescriptor::M2m(d) = &out[0] else {
            panic!("expected M2m")
        };
        assert_eq!(d.parent_param, "contactGroupId");
        assert_eq!(d.target_param, "contactId");
        assert_eq!(d.parent_fk_field, "contact_group_id");
        assert_eq!(d.child_fk_field, "contact_id");
        assert_eq!(
            d.collection_path,
            "/api/contact-groups/{contactGroupId}/members"
        );
        assert_eq!(
            d.member_path,
            "/api/contact-groups/{contactGroupId}/members/{contactId}"
        );
    }

    #[test]
    fn expand_detects_junction_when_via_missing() {
        let entry = RawCombinedEntry {
            parent: "contact_group".to_string(),
            route: "/api/contact-groups/{id}".to_string(),
            children: vec![RawCombinedChild {
                name: "contact".to_string(),
                via: None,
                target: None,
                route: None,
            }],
        };
        let contact = make_entity("contact", &[]);
        let contact_group = make_entity("contact_group", &[]);
        let junction = make_entity(
            "contact_group_member",
            &[
                ("contact_id", "contact.id"),
                ("contact_group_id", "contact_group.id"),
            ],
        );
        let out = expand_combined_routes(&[entry], &[contact, contact_group, junction]).unwrap();
        let CombinedDescriptor::M2m(d) = &out[0] else {
            panic!("expected M2m")
        };
        assert_eq!(d.junction, "contact_group_member");
        assert_eq!(
            d.collection_path,
            "/api/contact-groups/{contactGroupId}/contacts"
        );
    }

    #[test]
    fn expand_errors_on_ambiguous_junction() {
        let entry = RawCombinedEntry {
            parent: "contact_group".to_string(),
            route: "/api/contact-groups/{id}".to_string(),
            children: vec![RawCombinedChild {
                name: "contact".to_string(),
                via: None,
                target: None,
                route: None,
            }],
        };
        let contact = make_entity("contact", &[]);
        let contact_group = make_entity("contact_group", &[]);
        let j1 = make_entity(
            "j1",
            &[
                ("contact_id", "contact.id"),
                ("contact_group_id", "contact_group.id"),
            ],
        );
        let j2 = make_entity(
            "j2",
            &[
                ("contact_id", "contact.id"),
                ("contact_group_id", "contact_group.id"),
            ],
        );
        let err = expand_combined_routes(&[entry], &[contact, contact_group, j1, j2]).unwrap_err();
        assert!(matches!(err, CombinedRoutesError::AmbiguousJunction { .. }));
    }

    struct OccDirectService;

    #[async_trait::async_trait]
    impl DynamicService for OccDirectService {
        async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
            match method {
                "findById" | "find_by_id" | "find" => {
                    Ok(json!({ "id": "msg-1", "conversation_id": "conv-1" }))
                }
                "update" | "delete" => {
                    let has_token = args
                        .get(IF_MATCH_ARG)
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.is_empty());
                    if !has_token {
                        return Err(ServiceError::PreconditionRequired(
                            "If-Match header required for this mutation".to_string(),
                        ));
                    }
                    Ok(json!({ "id": "msg-1", "conversation_id": "conv-1" }))
                }
                other => Err(ServiceError::UnknownMethod(other.to_string())),
            }
        }
    }

    fn direct_fk_params() -> HashMap<String, String> {
        let mut params = HashMap::new();
        params.insert("conversation_id".to_string(), "conv-1".to_string());
        params.insert("id".to_string(), "msg-1".to_string());
        params
    }

    fn put_request(if_match: Option<&str>) -> Request {
        use axum::body::Body;
        let mut builder = Request::builder()
            .method("PUT")
            .uri("/")
            .header("content-type", "application/json");
        if let Some(token) = if_match {
            builder = builder.header("if-match", token);
        }
        builder
            .body(Body::from(json!({ "body": "edited" }).to_string()))
            .unwrap()
    }

    fn if_match_headers(if_match: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(token) = if_match {
            headers.insert("if-match", token.parse().unwrap());
        }
        headers
    }

    #[tokio::test]
    async fn direct_fk_update_without_if_match_is_428_under_occ() {
        let resp = direct_fk_update(
            Arc::new(OccDirectService),
            direct_fk_params(),
            put_request(None),
            "conversation_id".to_string(),
            "conversation_id".to_string(),
            "message".to_string(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[tokio::test]
    async fn direct_fk_update_threads_if_match_token_to_the_service() {
        let resp = direct_fk_update(
            Arc::new(OccDirectService),
            direct_fk_params(),
            put_request(Some("2020-01-01T00:00:00.000Z")),
            "conversation_id".to_string(),
            "conversation_id".to_string(),
            "message".to_string(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn direct_fk_delete_without_if_match_is_428_under_occ() {
        let resp = direct_fk_delete(
            Arc::new(OccDirectService),
            direct_fk_params(),
            if_match_headers(None),
            "conversation_id".to_string(),
            "conversation_id".to_string(),
            "message".to_string(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[tokio::test]
    async fn direct_fk_delete_threads_if_match_token_to_the_service() {
        let resp = direct_fk_delete(
            Arc::new(OccDirectService),
            direct_fk_params(),
            if_match_headers(Some("2020-01-01T00:00:00.000Z")),
            "conversation_id".to_string(),
            "conversation_id".to_string(),
            "message".to_string(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
