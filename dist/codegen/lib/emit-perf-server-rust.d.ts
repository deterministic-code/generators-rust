import type { ParsedSettings } from "@deterministic-code/generator-sdk/read-settings";
import type { EmittedFile } from "@deterministic-code/generator-sdk/codegen/lib/routes-emit-types";
interface DatasourceData {
    types?: unknown;
}
interface PerfServerRustEmitInput {
    inputs: {
        all: () => Promise<{
            datasourceYamlText: string;
        }>;
    };
    settings: ParsedSettings;
}
export declare function emitPerfServerRust({ datasourceData, }?: {
    datasourceData?: DatasourceData;
}): EmittedFile;
export declare const entriesNative = true;
/** The perf_server.rs bin lives under src/bin, outside the crate's mod.rs graph — so no rust module wiring. */
export declare const rustModuleWiring = false;
/** Self-describing catalog `perf_server` (rust): the perf-server bin plus its Cargo.toml `[[bin]]` marked-block patch. `--output` is the crate root, so the bin self-encodes its `src/bin/` path and the Cargo.toml patch lands at the root — no artifact-root knob. */
export declare function emit({ inputs, settings }: PerfServerRustEmitInput): Promise<{
    entries: import("@deterministic-code/generator-sdk/codegen/lib/emit-result").EmitEntry[];
}>;
export {};
