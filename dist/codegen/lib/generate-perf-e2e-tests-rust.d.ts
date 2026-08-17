import type { GeneratedFile } from "@deterministic-code/generator-sdk/codegen/lib/routes-generate-types";
export declare function generatePerfE2eTestRust(): GeneratedFile;
/** The perf e2e client is a `tests/` integration binary, outside the crate's mod.rs graph — no rust module wiring. */
export declare const rustModuleWiring = false;
/** Self-describing catalog `perf_e2e_tests` (rust): the perf e2e client binary that replays performance-plan.yaml against a running backend. The `--output` already resolves to the crate's `tests` dir, so the file is placed by basename. */
export declare function generate(): Promise<{
    files: {
        path: string;
        content: string;
    }[];
}>;
