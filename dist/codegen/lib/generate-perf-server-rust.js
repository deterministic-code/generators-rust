import { parseDatasourceTypes } from "@deterministic-code/generator-sdk/codegen/lib/parse-datasource-types";
import { join } from "node:path";
import { CONTENT } from "@deterministic-code/generator-sdk/codegen/lib/generate-result";
import { perfServerRustEntries } from "@deterministic-code/generator-sdk/codegen/lib/migrate-perf-harness";
export function generatePerfServerRust({ datasourceData, } = {}) {
    if (!datasourceData || typeof datasourceData !== "object") {
        throw new Error("generatePerfServerRust: datasourceData is required (parsed datasource_types.yaml)");
    }
    if (!Array.isArray(datasourceData.types)) {
        throw new Error("generatePerfServerRust: datasourceData.types must be an array — malformed datasource_types.yaml");
    }
    const content = `use deterministic::run::{run, RunConfig};

#[tokio::main]
async fn main() {
    if let Err(err) = run(RunConfig::from_env()).await {
        eprintln!("perf-server: {err}");
        std::process::exit(1);
    }
}
`;
    return { path: "perf_server.rs", content };
}
export const entriesNative = true;
/** The perf_server.rs bin lives under src/bin, outside the crate's mod.rs graph — so no rust module wiring. */
export const rustModuleWiring = false;
/** Self-describing catalog `perf_server` (rust): the perf-server bin plus its Cargo.toml `[[bin]]` marked-block patch. `--output` is the crate root, so the bin self-encodes its `src/bin/` path and the Cargo.toml patch lands at the root — no artifact-root knob. */
export async function generate({ inputs, settings }) {
    const { datasourceYamlText } = await inputs.all();
    const file = generatePerfServerRust({
        datasourceData: parseDatasourceTypes(datasourceYamlText, settings),
    });
    return {
        entries: [
            {
                kind: CONTENT,
                filename: join("src", "bin", file.path),
                contents: file.content,
            },
            ...perfServerRustEntries(),
        ],
    };
}
