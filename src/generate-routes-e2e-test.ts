import { datasourceSettings } from "./common/datasource-settings.ts";
import type { GenerateContext } from "./common/generate-context.ts";
import { content, type GenerateEntry } from "./common/generate-entry.ts";
import { rustNaming, rustRouteNaming } from "./common/naming.ts";
import {
  DATASOURCE_TYPES_YAML,
  parseDatasourceTypes,
  type DatasourceType,
} from "./common/parse-datasource-types.ts";

const STANDARD = new Set(["id", "uuid", "created", "updated"]);

const jsonSample = (type: string): string => {
  switch (type) {
    case "boolean":
      return "true";
    case "number":
    case "integer":
    case "smallinteger":
    case "biginteger":
    case "float":
    case "reference":
      return "1";
    case "datetime":
      return `"2020-01-01T00:00:00.000Z"`;
    case "uuid":
      return `"00000000-0000-0000-0000-000000000001"`;
    default:
      return `"x"`;
  }
};

const samplePayload = (table: DatasourceType): string => {
  const parts = table.fields
    .filter(
      (f) =>
        !STANDARD.has(f.name) &&
        !f.isNullable &&
        f.hasDefault !== true &&
        f.references === undefined,
    )
    .map((f) => `"${f.name}":${jsonSample(f.type)}`);
  return `{${parts.join(",")}}`;
};

const setupFn = (entity: string, segment: string): string =>
  `fn setup_${entity}_router() -> axum::Router {
    let svc: std::sync::Arc<dyn DynamicService> = std::sync::Arc::new(
        GenericCrudService::new(std::sync::Arc::new(InMemoryCrudRepository::new())),
    );
    let registry = ServiceRegistryBuilder::new()
        .register("${entity}", svc)
        .build();
    let specs = vec![
        GenericRouteSpec {
            route_name: "list_${entity}".to_string(),
            path: "/api/${segment}".to_string(),
            method: HttpMethod::Get,
            service: "${entity}".to_string(),
            service_method: "findAll".to_string(),
            response_format: Some(ResponseFormat::Items),
            status_code: None,
            aliases: vec![],
        },
        GenericRouteSpec {
            route_name: "get_${entity}_by_id".to_string(),
            path: "/api/${segment}/:id".to_string(),
            method: HttpMethod::Get,
            service: "${entity}".to_string(),
            service_method: "findById".to_string(),
            response_format: Some(ResponseFormat::Item),
            status_code: None,
            aliases: vec![],
        },
        GenericRouteSpec {
            route_name: "create_${entity}".to_string(),
            path: "/api/${segment}".to_string(),
            method: HttpMethod::Post,
            service: "${entity}".to_string(),
            service_method: "create".to_string(),
            response_format: Some(ResponseFormat::Item),
            status_code: None,
            aliases: vec![],
        },
        GenericRouteSpec {
            route_name: "delete_${entity}_by_id".to_string(),
            path: "/api/${segment}/:id".to_string(),
            method: HttpMethod::Delete,
            service: "${entity}".to_string(),
            service_method: "delete".to_string(),
            response_format: None,
            status_code: None,
            aliases: vec![],
        },
    ];
    let (router, _report) = build_router(&specs, &registry);
    router
}`;

const regularTests = (
  entity: string,
  pascal: string,
  segment: string,
  payload: string,
  missing: string,
): string => `#[tokio::test]
async fn ${entity}_list_returns_200() {
    let router = setup_${entity}_router();
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
async fn ${entity}_post_accepts_sample_payload() {
    let router = setup_${entity}_router();
    let req = HttpRequest::builder()
        .method("POST")
        .uri("/api/${segment}")
        .header("content-type", "application/json")
        .body(Body::from(r#"${payload}"#))
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert!(
        res.status() == StatusCode::CREATED || res.status() == StatusCode::BAD_REQUEST,
        "${pascal} POST expected 201 or 400, got {}",
        res.status()
    );
}

#[tokio::test]
async fn ${entity}_get_missing_returns_404() {
    let router = setup_${entity}_router();
    let req = HttpRequest::builder()
        .method("GET")
        .uri("/api/${segment}/${missing}")
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
async fn ${entity}_delete_missing_returns_non_5xx() {
    let router = setup_${entity}_router();
    let req = HttpRequest::builder()
        .method("DELETE")
        .uri("/api/${segment}/${missing}")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert!(
        res.status().as_u16() < 500,
        "${pascal} DELETE missing expected non-5xx, got {}",
        res.status()
    );
}`;

const readonlyTests = (
  entity: string,
  pascal: string,
  segment: string,
  missing: string,
): string => `#[tokio::test]
async fn ${entity}_list_returns_200() {
    let router = setup_${entity}_router();
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
async fn ${entity}_get_missing_returns_404() {
    let router = setup_${entity}_router();
    let req = HttpRequest::builder()
        .method("GET")
        .uri("/api/${segment}/${missing}")
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

export const generate = async (
  ctx: GenerateContext,
): Promise<GenerateEntry[]> => {
  if (!(await ctx.reader.exists(DATASOURCE_TYPES_YAML))) return [];
  const ds = datasourceSettings(ctx.settings);
  const naming = rustRouteNaming(ctx.settings);
  const names = rustNaming(ctx.settings);
  const tables = parseDatasourceTypes({
    yaml: await ctx.reader.read(DATASOURCE_TYPES_YAML),
    idType: ds.idType,
  }).filter((t) => t.datasourceType !== "many-to-many");
  const missing =
    ds.idType === "uuid" ? "00000000-0000-0000-0000-000000000000" : "99999";
  const setups = tables
    .map((t) => setupFn(t.name, naming.apiPath(t.name)))
    .join("\n\n");
  const tests = tables
    .map((t) => {
      const pascal = names.className(t.name);
      const segment = naming.apiPath(t.name);
      return t.datasourceType === "readonly-lookup"
        ? readonlyTests(t.name, pascal, segment, missing)
        : regularTests(t.name, pascal, segment, samplePayload(t), missing);
    })
    .join("\n\n");
  return [
    content(
      "app_routes_e2e.rs",
      `use axum::body::Body;
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
`,
    ),
  ];
};
