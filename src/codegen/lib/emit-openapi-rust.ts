import { layoutFor } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
import type { OpenApiDocument } from "@deterministic-code/generator-sdk/codegen/lib/openapi-types";
import type { EmittedFile } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit-types";

const layout = layoutFor({ language: "rust" });

const jsonObjectContent = {
  "application/json": { schema: { type: "object" } },
};
const okObjectResponse = { description: "OK", content: jsonObjectContent };
const notFoundResponse = { description: "Not Found" };
const idParameter = {
  name: "id",
  in: "path",
  required: true,
  schema: { type: "integer", format: "int64" },
};
const objectRequestBody = { required: true, content: jsonObjectContent };

interface SpecInfo {
  title?: string;
  version?: string;
  [key: string]: unknown;
}

interface BuiltSpec {
  info?: SpecInfo;
  [key: string]: unknown;
}

interface OpenApiDocOptions {
  title?: string;
  version?: string;
  entities?: string[];
  enrichedSpec?: OpenApiDocument | null;
}

interface ConformanceTestOptions {
  crateName?: string;
  entities?: string[];
  enrichedSpec?: OpenApiDocument | null;
}

function entityPathBase(entityName: string): string {
  return `/api/${layout.apiPath(entityName)}`;
}

function tagFor(entityName: string): string {
  return layout.apiPath(entityName);
}

function collectionOps(tag: string) {
  return {
    get: {
      tags: [tag],
      summary: `List ${tag}`,
      responses: {
        200: {
          description: "OK",
          content: {
            "application/json": {
              schema: { type: "array", items: { type: "object" } },
            },
          },
        },
      },
    },
    post: {
      tags: [tag],
      summary: `Create ${tag}`,
      requestBody: objectRequestBody,
      responses: {
        201: { description: "Created", content: jsonObjectContent },
      },
    },
  };
}

function memberOps(tag: string) {
  return {
    get: {
      tags: [tag],
      summary: `Get one ${tag}`,
      parameters: [idParameter],
      responses: { 200: okObjectResponse, 404: notFoundResponse },
    },
    put: {
      tags: [tag],
      summary: `Update ${tag}`,
      parameters: [idParameter],
      requestBody: objectRequestBody,
      responses: { 200: okObjectResponse, 404: notFoundResponse },
    },
    patch: {
      tags: [tag],
      summary: `Partially update ${tag}`,
      parameters: [idParameter],
      requestBody: objectRequestBody,
      responses: { 200: okObjectResponse, 404: notFoundResponse },
    },
    delete: {
      tags: [tag],
      summary: `Delete ${tag}`,
      parameters: [idParameter],
      responses: { 200: okObjectResponse, 404: notFoundResponse },
    },
  };
}

function buildEntityPaths(entityName: string) {
  const base = entityPathBase(entityName);
  const tag = tagFor(entityName);
  return {
    [base]: collectionOps(tag),
    [`${base}/{id}`]: memberOps(tag),
  };
}

export function buildOpenApiSpec({
  title = "API",
  version = "0.0.0",
  entities = [],
  enrichedSpec = null,
}: OpenApiDocOptions = {}): BuiltSpec {
  if (enrichedSpec && typeof enrichedSpec === "object") {
    const clone: BuiltSpec = JSON.parse(JSON.stringify(enrichedSpec));
    clone.info = { ...(clone.info ?? {}), title, version };
    return clone;
  }
  const sorted = [...entities].sort();
  const paths: Record<string, unknown> = {};
  for (const entity of sorted) {
    Object.assign(paths, buildEntityPaths(entity));
  }
  const tags = sorted.map((e) => ({ name: tagFor(e) }));
  return {
    openapi: "3.0.3",
    info: { title, version },
    tags,
    paths,
    components: { schemas: {} },
  };
}

function escapeRustStringLiteral(s: string): string {
  return s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n");
}

function conformancePaths(opts: ConformanceTestOptions): string[] {
  const { entities = [], enrichedSpec = null } = opts;
  if (
    enrichedSpec &&
    enrichedSpec.paths &&
    typeof enrichedSpec.paths === "object"
  ) {
    return Object.keys(enrichedSpec.paths).sort();
  }
  return [...entities].sort().flatMap((e) => {
    const base = entityPathBase(e);
    return [base, `${base}/{id}`];
  });
}

export function emitOpenApiConformanceTest(
  opts: ConformanceTestOptions = {},
): EmittedFile {
  const expectedPaths = conformancePaths(opts)
    .map((p) => `        ${JSON.stringify(p)},`)
    .join("\n");
  const crateModule = (opts.crateName ?? "consumer").replace(/-/g, "_");
  const content = `use ${crateModule}::routes::openapi;
use reqwest::StatusCode;
use serde_json::Value;
use tokio::net::TcpListener;

async fn serve_openapi() -> String {
    let router = openapi::router();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{}", addr)
}

#[tokio::test]
async fn openapi_endpoint_returns_valid_spec_document() {
    let base = serve_openapi().await;
    let resp = reqwest::get(format!("{}/openapi.json", base))
        .await
        .expect("GET /openapi.json");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_string()),
        Some("application/json".to_string())
    );
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body.get("openapi").and_then(|v| v.as_str()), Some("3.0.3"));
    assert!(body.get("info").is_some(), "info missing");
    assert!(body.get("paths").is_some(), "paths missing");
}

#[tokio::test]
async fn openapi_paths_cover_every_emitted_entity() {
    let base = serve_openapi().await;
    let body: Value = reqwest::get(format!("{}/openapi.json", base))
        .await
        .expect("GET")
        .json()
        .await
        .expect("json");
    let paths = body.get("paths").and_then(|v| v.as_object()).expect("paths");
    let expected: &[&str] = &[
${expectedPaths}
    ];
    for path in expected {
        assert!(
            paths.contains_key(*path),
            "openapi.paths missing entry for {}",
            path
        );
    }
}
`;
  return { path: "openapi_conformance.rs", content };
}

export function emitOpenApiRouter({
  title,
  version,
  entities = [],
  enrichedSpec = null,
}: OpenApiDocOptions = {}): EmittedFile {
  const spec = buildOpenApiSpec({ title, version, entities, enrichedSpec });
  const specJson = JSON.stringify(spec);
  const content = `use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::get;

const OPENAPI_JSON: &str = "${escapeRustStringLiteral(specJson)}";

pub fn router() -> axum::Router {
    axum::Router::new().route("/openapi.json", get(serve_openapi))
}

async fn serve_openapi() -> Response {
    (
        [(CONTENT_TYPE, "application/json")],
        OPENAPI_JSON.to_string(),
    )
        .into_response()
}
`;
  return { path: "openapi.rs", content };
}
