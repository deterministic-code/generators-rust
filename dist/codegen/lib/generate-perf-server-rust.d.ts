import type { ParsedSettings } from "@deterministic-code/generator-sdk/read-settings";
import type { GeneratedFile } from "@deterministic-code/generator-sdk/codegen/lib/routes-generate-types";
interface DatasourceData {
    types?: unknown;
}
interface PerfServerRustGenerateInput {
    inputs: {
        all: () => Promise<{
            datasourceYamlText: string;
        }>;
    };
    settings: ParsedSettings;
}
export declare function generatePerfServerRust({ datasourceData, }?: {
    datasourceData?: DatasourceData;
}): GeneratedFile;
export declare const entriesNative = true;
/** The perf_server.rs bin lives under src/bin, outside the crate's mod.rs graph — so no rust module wiring. */
export declare const rustModuleWiring = false;
/** Self-describing catalog `perf_server` (rust): the perf-server bin plus its Cargo.toml `[[bin]]` marked-block patch. `--output` is the crate root, so the bin self-encodes its `src/bin/` path and the Cargo.toml patch lands at the root — no artifact-root knob. */
export declare function generate({ inputs, settings }: PerfServerRustGenerateInput): Promise<{
    entries: import("@deterministic-code/generator-sdk/codegen/lib/generate-result").GenerateEntry[];
}>;
export {};
