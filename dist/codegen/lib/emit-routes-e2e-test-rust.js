import { emitRouteE2eFiles, dispatchRouteE2eStep, routesStepEmit, } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit";
import { layoutFor, namesFor, } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
import { STANDARD_COLUMNS } from "@deterministic-code/generator-sdk/codegen/lib/integration-test-spec";
import { loadFieldTypeCatalog } from "@deterministic-code/generator-sdk/lib/field-type-catalog";
import { fieldConverterFor } from "@deterministic-code/generator-sdk/lib/field-converter";
const RUST_CONVERTER = fieldConverterFor({
    targetKind: "language",
    target: "rust",
    catalog: await loadFieldTypeCatalog(),
    datetimeRepr: "native",
});
const layout = layoutFor({ language: "rust" });
const names = namesFor({ language: "rust" });
function classifyEntity(typeDef) {
    if (typeDef?.datasource_type === "many-to-many")
        return "m2m";
    if (typeDef?.datasource_type === "readonly-lookup")
        return "readonly";
    return "regular";
}
function fieldsOf(typeDef) {
    return Array.isArray(typeDef?.fields) ? typeDef.fields : [];
}
function pathSegmentFor(entityName) {
    return layout.apiPath(entityName);
}
function buildSamplePayloadJson(typeDef) {
    const standard = new Set(STANDARD_COLUMNS);
    const parts = [];
    for (const entry of fieldsOf(typeDef)) {
        const [name, def] = Object.entries(entry)[0];
        if (standard.has(name))
            continue;
        if (def?.is_nullable)
            continue;
        if (def?.default_value !== undefined && def?.default_value !== null)
            continue;
        if (typeof def?.references === "string")
            continue;
        parts.push(`"${name}":${RUST_CONVERTER.jsonSample(def)}`);
    }
    return `{${parts.join(",")}}`;
}
function pascalize(name) {
    return names.className(name);
}
function emitSetupFn(entityName) {
    const segment = pathSegmentFor(entityName);
    return `fn setup_${entityName}_router() -> axum::Router {
    let svc: std::sync::Arc<dyn DynamicService> = std::sync::Arc::new(
        GenericCrudService::new(std::sync::Arc::new(InMemoryCrudRepository::new())),
    );
    let registry = ServiceRegistryBuilder::new()
        .register("${entityName}", svc)
        .build();
    let specs = vec![
        GenericRouteSpec {
            route_name: "list_${entityName}".to_string(),
            path: "/api/${segment}".to_string(),
            method: HttpMethod::Get,
            service: "${entityName}".to_string(),
            service_method: "findAll".to_string(),
            response_format: Some(ResponseFormat::Items),
            status_code: None,
            aliases: vec![],
        },
        GenericRouteSpec {
            route_name: "get_${entityName}_by_id".to_string(),
            path: "/api/${segment}/:id".to_string(),
            method: HttpMethod::Get,
            service: "${entityName}".to_string(),
            service_method: "findById".to_string(),
            response_format: Some(ResponseFormat::Item),
            status_code: None,
            aliases: vec![],
        },
        GenericRouteSpec {
            route_name: "create_${entityName}".to_string(),
            path: "/api/${segment}".to_string(),
            method: HttpMethod::Post,
            service: "${entityName}".to_string(),
            service_method: "create".to_string(),
            response_format: Some(ResponseFormat::Item),
            status_code: None,
            aliases: vec![],
        },
        GenericRouteSpec {
            route_name: "delete_${entityName}_by_id".to_string(),
            path: "/api/${segment}/:id".to_string(),
            method: HttpMethod::Delete,
            service: "${entityName}".to_string(),
            service_method: "delete".to_string(),
            response_format: None,
            status_code: None,
            aliases: vec![],
        },
    ];
    let (router, _report) = build_router(&specs, &registry);
    router
}`;
}
function emitRegularEntityTests(entityName, typeDef, missingSegment) {
    const pascal = pascalize(entityName);
    const segment = pathSegmentFor(entityName);
    const samplePayload = buildSamplePayloadJson(typeDef);
    return `#[tokio::test]
async fn ${entityName}_list_returns_200() {
    let router = setup_${entityName}_router();
    let req = HttpRequest::builder()
        .method("GET")
        .uri("/api/${segment}")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "${pascal} list expected 200, got {}",
        res.status()
    );
}

#[tokio::test]
async fn ${entityName}_post_accepts_sample_payload() {
    let router = setup_${entityName}_router();
    let req = HttpRequest::builder()
        .method("POST")
        .uri("/api/${segment}")
        .header("content-type", "application/json")
        .body(Body::from(r#"${samplePayload}"#))
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert!(
        res.status() == StatusCode::CREATED || res.status() == StatusCode::BAD_REQUEST,
        "${pascal} POST expected 201 or 400, got {}",
        res.status()
    );
}

#[tokio::test]
async fn ${entityName}_get_missing_returns_404() {
    let router = setup_${entityName}_router();
    let req = HttpRequest::builder()
        .method("GET")
        .uri("/api/${segment}/${missingSegment}")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "${pascal} GET missing expected 404, got {}",
        res.status()
    );
}

#[tokio::test]
async fn ${entityName}_delete_missing_returns_non_5xx() {
    let router = setup_${entityName}_router();
    let req = HttpRequest::builder()
        .method("DELETE")
        .uri("/api/${segment}/${missingSegment}")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert!(
        res.status().as_u16() < 500,
        "${pascal} DELETE missing expected non-5xx, got {}",
        res.status()
    );
}`;
}
function emitReadonlyEntityTests(entityName, missingSegment) {
    const pascal = pascalize(entityName);
    const segment = pathSegmentFor(entityName);
    return `#[tokio::test]
async fn ${entityName}_list_returns_200() {
    let router = setup_${entityName}_router();
    let req = HttpRequest::builder()
        .method("GET")
        .uri("/api/${segment}")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "${pascal} list expected 200, got {}",
        res.status()
    );
}

#[tokio::test]
async fn ${entityName}_get_missing_returns_404() {
    let router = setup_${entityName}_router();
    let req = HttpRequest::builder()
        .method("GET")
        .uri("/api/${segment}/${missingSegment}")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "${pascal} GET missing expected 404, got {}",
        res.status()
    );
}`;
}
export function emitAppE2ETest({ datasourceData, datasourceSettings, }) {
    /** A uuid project's "missing row → 404" probe must hit a valid uuid — a hardcoded 99999 fails path-param parsing (400) instead; integer/string projects keep the 99999 sentinel. */
    const missingSegment = datasourceSettings
        ? datasourceSettings.missingIdSentinel()
        : "99999";
    const entities = [];
    for (const entry of datasourceData?.types ?? []) {
        const [name, def] = Object.entries(entry)[0];
        const kind = classifyEntity(def);
        if (kind === "m2m")
            continue;
        entities.push({ name, def, kind });
    }
    const setups = entities.map((e) => emitSetupFn(e.name)).join("\n\n");
    const tests = entities
        .map((e) => e.kind === "readonly"
        ? emitReadonlyEntityTests(e.name, missingSegment)
        : emitRegularEntityTests(e.name, e.def, missingSegment))
        .join("\n\n");
    const content = `use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use tower::ServiceExt;

use deterministic::loaders::{GenericRouteSpec, HttpMethod, ResponseFormat};
use deterministic::repositories::inmemory::InMemoryCrudRepository;
use deterministic::services::DynamicService;
use deterministic::{
    build_router, GenericCrudService, ServiceRegistryBuilder,
};

${setups}

${tests}
`;
    return { path: "app_routes_e2e.rs", content };
}
/** Catalog `routes_e2e_test` step (rust). */
export const emit = (ctx) => routesStepEmit({
    dispatchStep: dispatchRouteE2eStep,
    emitter: { createEmitter },
    language: "rust",
}, ctx);
export const createEmitter = () => ({
    emit: (config) => emitRouteE2eFiles({ ...config, primitives: { emitAppE2ETest } }),
});
