use std::path::PathBuf;

use deterministic::loaders::{
    load_datasource_types, load_routes, load_services, load_settings_config, load_view_types,
};

fn samples_dir() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .expect("rust crate has parent")
        .join("samples")
}

fn list_samples() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries =
        std::fs::read_dir(samples_dir()).unwrap_or_else(|e| panic!("read samples dir: {}", e));
    for entry in entries {
        let entry = entry.unwrap();
        let det = entry.path().join("deterministic");
        if det.is_dir() {
            out.push(det);
        }
    }
    assert!(!out.is_empty(), "no samples/*/deterministic/ found");
    out
}

#[test]
fn every_sample_settings_yaml_parses() {
    for det in list_samples() {
        let path = det.join("settings.yaml");
        let cfg = load_settings_config(&path)
            .unwrap_or_else(|e| panic!("settings.yaml at {:?}: {}", path, e));
        let _ = cfg.datasource.pluralize_table_names;
    }
}

#[test]
fn every_sample_services_yaml_parses() {
    for det in list_samples() {
        let path = det.join("services.yaml");
        let services =
            load_services(&path).unwrap_or_else(|e| panic!("services.yaml at {:?}: {}", path, e));
        for svc in &services {
            assert!(!svc.name.is_empty(), "{:?} has empty service name", path);
        }
    }
}

#[test]
fn every_sample_routes_yaml_parses() {
    for det in list_samples() {
        let path = det.join("routes.yaml");
        let routes =
            load_routes(&path).unwrap_or_else(|e| panic!("routes.yaml at {:?}: {}", path, e));
        for r in &routes.generic {
            assert!(
                !r.path.is_empty(),
                "{:?} generic route '{}' has empty path",
                path,
                r.route_name
            );
            assert!(
                !r.service.is_empty(),
                "{:?} generic route '{}' has empty service",
                path,
                r.route_name
            );
        }
        for r in &routes.custom {
            assert!(
                !r.module.is_empty(),
                "{:?} custom route '{}' has empty module",
                path,
                r.route_name
            );
        }
    }
}

#[test]
fn every_sample_datasource_types_yaml_parses() {
    for det in list_samples() {
        let path = det.join("datasource_types.yaml");
        let doc = load_datasource_types(&path)
            .unwrap_or_else(|e| panic!("datasource_types.yaml at {:?}: {}", path, e));
        for e in &doc.types {
            assert!(!e.name.is_empty(), "{:?} entity has empty name", path);
            for f in &e.fields {
                assert!(
                    !f.r#type.is_empty(),
                    "{:?} entity '{}' field '{}' has empty type",
                    path,
                    e.name,
                    f.name
                );
            }
        }
    }
}

#[test]
fn every_sample_view_types_yaml_parses() {
    for det in list_samples() {
        let path = det.join("view_types.yaml");
        let doc = load_view_types(&path)
            .unwrap_or_else(|e| panic!("view_types.yaml at {:?}: {}", path, e));
        for v in &doc.types {
            assert!(!v.name.is_empty(), "{:?} view has empty name", path);
        }
    }
}

#[test]
fn notifications_backend_snapshot() {
    let det = samples_dir()
        .join("notifications-backend")
        .join("deterministic");
    let ds = load_datasource_types(&det.join("datasource_types.yaml")).unwrap();
    let names: Vec<&str> = ds.types.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"notification"), "names={:?}", names);
    assert!(names.contains(&"notification_type"), "names={:?}", names);
    let nt = ds
        .types
        .iter()
        .find(|t| t.name == "notification_type")
        .unwrap();
    assert_eq!(nt.datasource_type.as_deref(), Some("readonly-lookup"));
    assert!(nt.skip_migrations);
    let pk_field = nt
        .fields
        .iter()
        .find(|f| f.primary_key)
        .expect("readonly-lookup has a PK");
    assert_eq!(pk_field.name, "name");

    let routes = load_routes(&det.join("routes.yaml")).unwrap();
    assert!(
        !routes.stubs.iter().any(|s| s.route_name.contains("_by_")),
        "byField shorthand entries must be recognised and dropped, not parsed as stubs"
    );
}
