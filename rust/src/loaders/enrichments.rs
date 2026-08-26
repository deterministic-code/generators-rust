use std::collections::HashMap;

use super::datasource_types::{DatasourceTypeDef, FieldDef};

#[derive(Debug, Clone, PartialEq)]
pub struct EnrichmentSpec {
    pub fk_column: String,
    pub prefix: String,
    pub target_table: String,
    pub new_field: String,
    pub target_is_readonly_lookup: bool,
    pub replace_fk: bool,
    pub suffix_field: Option<String>,
}

pub fn compute_enrichments_for_entity(
    entity: &DatasourceTypeDef,
    entities_by_name: &HashMap<&str, &DatasourceTypeDef>,
) -> Vec<EnrichmentSpec> {
    let mut out = Vec::new();
    for field in &entity.fields {
        if !field.name.ends_with("_id") {
            continue;
        }
        if field.r#type != "number" {
            continue;
        }
        let Some(target_table) = parse_fk_reference(field) else {
            continue;
        };
        let Some(target) = entities_by_name.get(target_table.as_str()) else {
            continue;
        };
        if !target_is_enrichable(target) {
            continue;
        }
        let prefix = field
            .name
            .strip_suffix("_id")
            .unwrap_or(field.name.as_str())
            .to_string();
        let new_field = format!("{}_name", prefix);
        out.push(EnrichmentSpec {
            fk_column: field.name.clone(),
            prefix,
            new_field,
            target_is_readonly_lookup: target.datasource_type.as_deref() == Some("readonly-lookup"),
            target_table,
            replace_fk: false,
            suffix_field: None,
        });
    }
    out
}

fn parse_fk_reference(field: &FieldDef) -> Option<String> {
    let ref_str = field.references.as_deref()?;
    let dot = ref_str.find('.')?;
    if dot == 0 {
        return None;
    }
    let (table, rest) = ref_str.split_at(dot);
    let column = &rest[1..];
    if column != "id" {
        return None;
    }
    Some(table.to_string())
}

fn target_is_enrichable(target: &DatasourceTypeDef) -> bool {
    if target.datasource_type.as_deref() == Some("readonly-lookup") {
        return true;
    }
    let Some(name_field) = target.fields.iter().find(|f| f.name == "name") else {
        return false;
    };
    if name_field.r#type != "string" {
        return false;
    }
    if !name_field.is_unique {
        return false;
    }
    if name_field.is_nullable {
        return false;
    }
    true
}

pub fn build_entities_by_name<'a>(
    entities: &'a [DatasourceTypeDef],
) -> HashMap<&'a str, &'a DatasourceTypeDef> {
    entities.iter().map(|e| (e.name.as_str(), e)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ds(name: &str, datasource_type: Option<&str>, fields: Vec<FieldDef>) -> DatasourceTypeDef {
        DatasourceTypeDef {
            name: name.to_string(),
            datasource_type: datasource_type.map(|s| s.to_string()),
            skip_migrations: false,
            optimistic_concurrency: None,
            target: Some("Crud".to_string()),
            fields,
            seeds: Vec::new(),
            ids: Vec::new(),
        }
    }

    fn fk(name: &str, references: &str) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            r#type: "number".to_string(),
            references: Some(references.to_string()),
            ..Default::default()
        }
    }

    fn str_field(name: &str, is_unique: bool, is_nullable: bool) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            r#type: "string".to_string(),
            is_unique,
            is_nullable,
            ..Default::default()
        }
    }

    #[test]
    fn enriches_fk_to_readonly_lookup() {
        let source = ds(
            "contact_source",
            Some("readonly-lookup"),
            vec![str_field("name", true, false)],
        );
        let contact = ds(
            "contact",
            None,
            vec![fk("contact_source_id", "contact_source.id")],
        );
        let entities = vec![source, contact];
        let map = build_entities_by_name(&entities);
        let enrichments = compute_enrichments_for_entity(&entities[1], &map);
        assert_eq!(enrichments.len(), 1);
        let e = &enrichments[0];
        assert_eq!(e.fk_column, "contact_source_id");
        assert_eq!(e.new_field, "contact_source_name");
        assert_eq!(e.target_table, "contact_source");
        assert!(e.target_is_readonly_lookup);
    }

    #[test]
    fn enriches_fk_when_target_has_unique_string_name() {
        let category = ds(
            "category",
            None,
            vec![
                str_field("name", true, false),
                str_field("description", false, true),
            ],
        );
        let item = ds("item", None, vec![fk("category_id", "category.id")]);
        let entities = vec![category, item];
        let map = build_entities_by_name(&entities);
        let enrichments = compute_enrichments_for_entity(&entities[1], &map);
        assert_eq!(enrichments.len(), 1);
        assert!(!enrichments[0].target_is_readonly_lookup);
    }

    #[test]
    fn ignores_fk_when_target_name_is_not_unique() {
        let cat = ds("cat", None, vec![str_field("name", false, false)]);
        let owner = ds("owner", None, vec![fk("cat_id", "cat.id")]);
        let entities = vec![cat, owner];
        let map = build_entities_by_name(&entities);
        assert!(compute_enrichments_for_entity(&entities[1], &map).is_empty());
    }

    #[test]
    fn ignores_fk_when_reference_not_id_column() {
        let other = ds(
            "other",
            Some("readonly-lookup"),
            vec![str_field("name", true, false)],
        );
        let me = ds("me", None, vec![fk("other_code", "other.code")]);
        let entities = vec![other, me];
        let map = build_entities_by_name(&entities);
        assert!(compute_enrichments_for_entity(&entities[1], &map).is_empty());
    }

    // Regression: a m2m junction whose FK targets an entity with a unique name must
    // enrich `<fk>_name` so a standalone-CRUD POST resolves `service_name` → `service_id`,
    // mirroring the TS runtime. A non-enrichable FK on the same junction stays a raw id.
    #[test]
    fn enriches_m2m_junction_fk_to_enrichable_target_only() {
        let service = ds("service", None, vec![str_field("name", true, false)]);
        let uri = ds("uri", None, vec![str_field("location", true, false)]);
        let junction = ds(
            "service_uri",
            Some("many-to-many"),
            vec![fk("service_id", "service.id"), fk("uri_id", "uri.id")],
        );
        let entities = vec![service, uri, junction];
        let map = build_entities_by_name(&entities);
        let enrichments = compute_enrichments_for_entity(&entities[2], &map);
        assert_eq!(enrichments.len(), 1);
        assert_eq!(enrichments[0].fk_column, "service_id");
        assert_eq!(enrichments[0].new_field, "service_name");
        assert_eq!(enrichments[0].target_table, "service");
    }

    // A readonly-lookup entity that itself carries an FK to another lookup enriches it too (matches the TS loader, which applies no entity-level datasource_type filter).
    #[test]
    fn enriches_readonly_lookup_holder_fk() {
        let foo = ds(
            "foo",
            Some("readonly-lookup"),
            vec![str_field("name", true, false)],
        );
        let holder = ds(
            "lookup_holder",
            Some("readonly-lookup"),
            vec![fk("foo_id", "foo.id")],
        );
        let entities = vec![foo, holder];
        let map = build_entities_by_name(&entities);
        let enrichments = compute_enrichments_for_entity(&entities[1], &map);
        assert_eq!(enrichments.len(), 1);
        assert_eq!(enrichments[0].new_field, "foo_name");
    }

    #[test]
    fn returns_empty_when_target_entity_missing() {
        let me = ds("me", None, vec![fk("ghost_id", "ghost.id")]);
        let entities = vec![me];
        let map = build_entities_by_name(&entities);
        assert!(compute_enrichments_for_entity(&entities[0], &map).is_empty());
    }
}
