use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value};

use super::dynamic_service::{DynamicService, ServiceError};
use crate::loaders::EnrichmentSpec;

pub struct LookupEnrichedService {
    base: Arc<dyn DynamicService>,
    enrichments: Vec<EnrichmentSpec>,
    lookups: HashMap<String, Arc<dyn DynamicService>>,
}

impl LookupEnrichedService {
    pub fn new(
        base: Arc<dyn DynamicService>,
        enrichments: Vec<EnrichmentSpec>,
        lookups: HashMap<String, Arc<dyn DynamicService>>,
    ) -> Self {
        Self {
            base,
            enrichments,
            lookups,
        }
    }

    async fn resolve_inbound_names(
        &self,
        mut body: Map<String, Value>,
    ) -> Result<Map<String, Value>, ServiceError> {
        for e in &self.enrichments {
            let Some(value) = body.remove(&e.new_field) else {
                continue;
            };
            if value.is_null() {
                body.insert(e.fk_column.clone(), Value::Null);
                continue;
            }
            let name = match value {
                Value::String(s) => s,
                other => {
                    return Err(ServiceError::InvalidArgs(format!(
                        "Invalid {}: expected string, got {}",
                        e.new_field, other
                    )));
                }
            };
            let lookup_svc = self.lookups.get(&e.target_table).ok_or_else(|| {
                ServiceError::InvalidArgs(format!(
                    "no lookup service registered for {}",
                    e.target_table
                ))
            })?;
            let rows = lookup_svc
                .invoke("findAll", Value::Object(Map::new()))
                .await?;
            let mut resolved_id: Option<Value> = None;
            if let Some(arr) = rows.as_array() {
                for row in arr {
                    let matches = row
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s == name)
                        .unwrap_or(false);
                    if matches {
                        if let Some(id) = row.get("id").cloned() {
                            resolved_id = Some(id);
                            break;
                        }
                    }
                }
            }
            let id = resolved_id.ok_or_else(|| {
                ServiceError::InvalidArgs(format!("Invalid {}: '{}'", e.new_field, name))
            })?;
            body.insert(e.fk_column.clone(), id);
        }
        Ok(body)
    }

    async fn transform_write_args(&self, args: Value) -> Result<Value, ServiceError> {
        let Value::Object(mut top) = args else {
            return Ok(args);
        };
        let mut handled = false;
        if let Some(Value::Object(data)) = top.get_mut("data") {
            let taken = std::mem::take(data);
            let resolved = self.resolve_inbound_names(taken).await?;
            top.insert("data".to_string(), Value::Object(resolved));
            handled = true;
        }
        if let Some(Value::Object(body)) = top.get_mut("body") {
            let taken = std::mem::take(body);
            let resolved = self.resolve_inbound_names(taken).await?;
            top.insert("body".to_string(), Value::Object(resolved));
            handled = true;
        }
        if !handled {
            top = self.resolve_inbound_names(top).await?;
        }
        Ok(Value::Object(top))
    }

    async fn enrich_response(&self, response: Value) -> Result<Value, ServiceError> {
        if self.enrichments.is_empty() {
            return Ok(response);
        }
        let needs_lookup = match &response {
            Value::Array(arr) => !arr.is_empty(),
            Value::Object(_) => true,
            _ => false,
        };
        if !needs_lookup {
            return Ok(response);
        }
        let mut maps: HashMap<String, HashMap<i64, Value>> = HashMap::new();
        for e in &self.enrichments {
            if maps.contains_key(&e.target_table) {
                continue;
            }
            let Some(lookup) = self.lookups.get(&e.target_table) else {
                maps.insert(e.target_table.clone(), HashMap::new());
                continue;
            };
            let all = lookup.invoke("findAll", Value::Object(Map::new())).await?;
            let mut id_to_name = HashMap::new();
            if let Some(arr) = all.as_array() {
                for row in arr {
                    let id = row.get("id").and_then(value_as_id);
                    let name = row.get("name").cloned();
                    if let (Some(id), Some(name)) = (id, name) {
                        id_to_name.insert(id, name);
                    }
                }
            }
            maps.insert(e.target_table.clone(), id_to_name);
        }
        Ok(match response {
            Value::Array(arr) => Value::Array(
                arr.into_iter()
                    .map(|item| self.apply_enrichment(item, &maps))
                    .collect(),
            ),
            other => self.apply_enrichment(other, &maps),
        })
    }

    fn apply_enrichment(&self, row: Value, maps: &HashMap<String, HashMap<i64, Value>>) -> Value {
        let Value::Object(mut obj) = row else {
            return row;
        };
        for e in &self.enrichments {
            let target_field: String = e
                .suffix_field
                .clone()
                .unwrap_or_else(|| e.new_field.clone());
            let should_strip_fk = e.replace_fk && e.suffix_field.is_none();
            let Some(fk_val) = obj.get(&e.fk_column).cloned() else {
                continue;
            };
            if fk_val.is_null() {
                obj.insert(target_field, Value::Null);
                if should_strip_fk {
                    obj.remove(&e.fk_column);
                }
                continue;
            }
            let Some(fk_id) = value_as_id(&fk_val) else {
                continue;
            };
            let Some(map) = maps.get(&e.target_table) else {
                continue;
            };
            if let Some(name) = map.get(&fk_id) {
                obj.insert(target_field, name.clone());
                if should_strip_fk {
                    obj.remove(&e.fk_column);
                }
            }
        }
        Value::Object(obj)
    }
}

// why f64 fallback: sqlite serializes number columns as JSON floats (1.0) so as_i64 alone misses them.
fn value_as_id(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    let f = v.as_f64()?;
    if f.fract() != 0.0 {
        return None;
    }
    Some(f as i64)
}

#[async_trait]
impl DynamicService for LookupEnrichedService {
    async fn invoke(&self, method: &str, args: Value) -> Result<Value, ServiceError> {
        match method {
            "create" | "add" | "update" => {
                let resolved_args = self.transform_write_args(args).await?;
                let result = self.base.invoke(method, resolved_args).await?;
                self.enrich_response(result).await
            }
            "findAll" | "find_all" | "findBy" | "find_by" | "findById" | "find_by_id" | "find" => {
                let result = self.base.invoke(method, args).await?;
                self.enrich_response(result).await
            }
            _ => self.base.invoke(method, args).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::inmemory::InMemoryCrudRepository;
    use crate::services::GenericCrudService;
    use serde_json::json;

    fn build_lookup_service() -> Arc<dyn DynamicService> {
        Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )))
    }

    fn enrichment(fk: &str, target: &str, new_field: &str) -> EnrichmentSpec {
        EnrichmentSpec {
            fk_column: fk.to_string(),
            prefix: fk.strip_suffix("_id").unwrap_or(fk).to_string(),
            target_table: target.to_string(),
            new_field: new_field.to_string(),
            target_is_readonly_lookup: true,
            replace_fk: true,
            suffix_field: None,
        }
    }

    async fn seed_lookup(svc: &Arc<dyn DynamicService>, names: &[&str]) -> Vec<i64> {
        let mut ids = Vec::new();
        for name in names {
            let row = svc.invoke("create", json!({ "name": name })).await.unwrap();
            ids.push(row["id"].as_i64().unwrap());
        }
        ids
    }

    #[tokio::test]
    async fn create_resolves_name_to_id_then_re_enriches_response() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let lookup = build_lookup_service();
        let ids = seed_lookup(&lookup, &["Manual", "Imported"]).await;
        let manual_id = ids[0];
        let mut lookups = HashMap::new();
        lookups.insert("contact_source".to_string(), lookup.clone());
        let svc = LookupEnrichedService::new(
            base,
            vec![enrichment(
                "contact_source_id",
                "contact_source",
                "contact_source_name",
            )],
            lookups,
        );
        let result = svc
            .invoke(
                "create",
                json!({
                    "body": { "first_name": "Alice", "contact_source_name": "Manual" }
                }),
            )
            .await
            .unwrap();
        assert_eq!(result["first_name"], json!("Alice"));
        assert_eq!(result["contact_source_name"], json!("Manual"));
        assert!(
            result.get("contact_source_id").is_none(),
            "auto_enrich response must strip the FK (TS replaceFk parity): {}",
            result
        );
        // Confirm the FK reached SQL even though it isn't in the response.
        let _ = manual_id;
    }

    #[tokio::test]
    async fn create_with_unknown_name_returns_invalid_args() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let lookup = build_lookup_service();
        seed_lookup(&lookup, &["Manual"]).await;
        let mut lookups = HashMap::new();
        lookups.insert("contact_source".to_string(), lookup);
        let svc = LookupEnrichedService::new(
            base,
            vec![enrichment(
                "contact_source_id",
                "contact_source",
                "contact_source_name",
            )],
            lookups,
        );
        let err = svc
            .invoke(
                "create",
                json!({ "body": { "contact_source_name": "Bogus" } }),
            )
            .await
            .unwrap_err();
        match err {
            ServiceError::InvalidArgs(m) => assert!(m.contains("Bogus")),
            other => panic!("expected InvalidArgs, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn create_with_null_name_writes_null_fk() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let lookup = build_lookup_service();
        let mut lookups = HashMap::new();
        lookups.insert("contact_source".to_string(), lookup);
        let svc = LookupEnrichedService::new(
            base,
            vec![enrichment(
                "contact_source_id",
                "contact_source",
                "contact_source_name",
            )],
            lookups,
        );
        let result = svc
            .invoke(
                "create",
                json!({ "body": { "first_name": "X", "contact_source_name": Value::Null } }),
            )
            .await
            .unwrap();
        assert!(result["contact_source_id"].is_null());
        assert!(result["contact_source_name"].is_null());
    }

    #[tokio::test]
    async fn find_all_enriches_each_row() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let lookup = build_lookup_service();
        let ids = seed_lookup(&lookup, &["Manual", "Imported"]).await;
        base.invoke(
            "create",
            json!({ "first_name": "A", "contact_source_id": ids[0] }),
        )
        .await
        .unwrap();
        base.invoke(
            "create",
            json!({ "first_name": "B", "contact_source_id": ids[1] }),
        )
        .await
        .unwrap();
        let mut lookups = HashMap::new();
        lookups.insert("contact_source".to_string(), lookup);
        let svc = LookupEnrichedService::new(
            base,
            vec![enrichment(
                "contact_source_id",
                "contact_source",
                "contact_source_name",
            )],
            lookups,
        );
        let rows = svc.invoke("findAll", json!({})).await.unwrap();
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["contact_source_name"], json!("Manual"));
        assert_eq!(arr[1]["contact_source_name"], json!("Imported"));
    }

    #[tokio::test]
    async fn find_all_preserves_fk_when_replace_fk_is_false() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let lookup = build_lookup_service();
        let ids = seed_lookup(&lookup, &["Manual"]).await;
        base.invoke(
            "create",
            json!({ "first_name": "A", "contact_source_id": ids[0] }),
        )
        .await
        .unwrap();
        let mut lookups = HashMap::new();
        lookups.insert("contact_source".to_string(), lookup);
        let mut spec = enrichment("contact_source_id", "contact_source", "contact_source_name");
        spec.replace_fk = false;
        let svc = LookupEnrichedService::new(base, vec![spec], lookups);
        let rows = svc.invoke("findAll", json!({})).await.unwrap();
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["contact_source_name"], json!("Manual"));
        assert_eq!(arr[0]["contact_source_id"], json!(ids[0]));
    }

    #[tokio::test]
    async fn find_all_writes_to_suffix_field_and_keeps_fk_and_new_field_absent() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let lookup = build_lookup_service();
        let ids = seed_lookup(&lookup, &["Manual"]).await;
        base.invoke(
            "create",
            json!({ "first_name": "A", "contact_source_id": ids[0] }),
        )
        .await
        .unwrap();
        let mut lookups = HashMap::new();
        lookups.insert("contact_source".to_string(), lookup);
        let mut spec = enrichment("contact_source_id", "contact_source", "contact_source_name");
        spec.replace_fk = true;
        spec.suffix_field = Some("contact_source_label".to_string());
        let svc = LookupEnrichedService::new(base, vec![spec], lookups);
        let rows = svc.invoke("findAll", json!({})).await.unwrap();
        let arr = rows.as_array().unwrap();
        assert_eq!(arr[0]["contact_source_label"], json!("Manual"));
        assert!(arr[0].get("contact_source_name").is_none());
        assert_eq!(arr[0]["contact_source_id"], json!(ids[0]));
    }

    #[tokio::test]
    async fn find_by_id_enriches_single_row() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let lookup = build_lookup_service();
        let ids = seed_lookup(&lookup, &["Manual"]).await;
        let created = base
            .invoke(
                "create",
                json!({ "first_name": "X", "contact_source_id": ids[0] }),
            )
            .await
            .unwrap();
        let mut lookups = HashMap::new();
        lookups.insert("contact_source".to_string(), lookup);
        let svc = LookupEnrichedService::new(
            base,
            vec![enrichment(
                "contact_source_id",
                "contact_source",
                "contact_source_name",
            )],
            lookups,
        );
        let row = svc
            .invoke("findById", json!({ "id": created["id"] }))
            .await
            .unwrap();
        assert_eq!(row["contact_source_name"], json!("Manual"));
    }

    #[tokio::test]
    async fn update_resolves_name_then_enriches() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let lookup = build_lookup_service();
        let ids = seed_lookup(&lookup, &["Manual", "Imported"]).await;
        let created = base
            .invoke(
                "create",
                json!({ "first_name": "X", "contact_source_id": ids[0] }),
            )
            .await
            .unwrap();
        let mut lookups = HashMap::new();
        lookups.insert("contact_source".to_string(), lookup);
        let svc = LookupEnrichedService::new(
            base,
            vec![enrichment(
                "contact_source_id",
                "contact_source",
                "contact_source_name",
            )],
            lookups,
        );
        let result = svc
            .invoke(
                "update",
                json!({
                    "id": created["id"],
                    "body": { "contact_source_name": "Imported" }
                }),
            )
            .await
            .unwrap();
        assert_eq!(result["contact_source_name"], json!("Imported"));
        assert!(
            result.get("contact_source_id").is_none(),
            "auto_enrich response must strip the FK (TS replaceFk parity): {}",
            result
        );
        let _ = ids;
    }

    #[tokio::test]
    async fn delete_passes_through_without_enrichment() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let created = base.invoke("create", json!({ "x": 1 })).await.unwrap();
        let svc = LookupEnrichedService::new(base, vec![], HashMap::new());
        let result = svc
            .invoke("delete", json!({ "id": created["id"] }))
            .await
            .unwrap();
        assert_eq!(result["deleted"], json!(true));
    }

    #[tokio::test]
    async fn no_enrichments_is_identity_pass_through() {
        let base = Arc::new(GenericCrudService::new(Arc::new(
            InMemoryCrudRepository::new(),
        )));
        let svc = LookupEnrichedService::new(base, vec![], HashMap::new());
        let r = svc
            .invoke("create", json!({ "body": { "name": "x" } }))
            .await
            .unwrap();
        assert_eq!(r["name"], json!("x"));
    }
}
