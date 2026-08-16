import { join } from "node:path";
import { rustMigrateBinBlock, rustSqlxDepLine, rustMigrateCopyContent, rustMigrateRuntimeCopyContent, } from "@deterministic-code/generator-sdk/lib/migrate-scripts-plan";
import { buildRustCargoTomlDepsBlock, rustDialectUseImportsForSetup, rustDialectUseImportsForRunners, rustDialectSqliteUrlHelperBlock, rustDialectDdlConstsBlock, rustDialectSetupDispatchBlock, rustDialectRunnerFnsBlock, rustDialectUpDispatchBlock, rustDialectRollbackFnsBlock, rustDialectDownDispatchBlock, } from "../../lib/emit-backend-app-rust.js";
import { dbFilePatches, entrypointPatch, markedEntry, dockerfileCopyPatches, } from "@deterministic-code/generator-sdk/codegen/lib/migrate-sibling-patches";
import { PATCH } from "@deterministic-code/generator-sdk/codegen/lib/emit-result";
import { content, gitkeepEntries, runnerTemplates, makeMigrateEmit, MIGRATE_DIR_FLAG, } from "@deterministic-code/generator-sdk/codegen/lib/migrate-emit-helpers";
import { COMBINED_FLAG } from "@deterministic-code/generator-sdk/codegen/lib/backend-lane";
import { withSqliteDialect } from "@deterministic-code/generator-sdk/codegen/lib/deterministic-project";
import { layoutForSettings } from "@deterministic-code/generator-sdk/codegen/lib/ts-codegen-naming";
const { read: runnerTemplate, composed } = runnerTemplates("rust");
async function renderRustMigrateCargoToml(dialects = [], migrateDir = ".") {
    const raw = await runnerTemplate("migrate-Cargo.toml");
    const withBins = raw.replace("[dependencies]", `${rustMigrateBinBlock(migrateDir)}\n\n[dependencies]`);
    return withBins.replace(/sqlx = \{ version = "0\.8", default-features = false, features = \[[^\]]*\] \}/, rustSqlxDepLine(dialects));
}
/** The migrate runner .rs sources with their per-dialect DIALECT_* sections composed at emit time (fillMarkedSections), so migrate emits its runners filled in rather than emit-then-patch. migrate_create.rs has no dialect sections. */
async function runnerFileEntries(migrateDir, dialects) {
    const useForSetup = rustDialectUseImportsForSetup(dialects);
    const useForRunners = rustDialectUseImportsForRunners(dialects);
    const sqliteUrl = rustDialectSqliteUrlHelperBlock(dialects);
    const setup = await composed("migrate_setup.rs", [
        ["DIALECT_USE_IMPORTS_RUST", useForSetup],
        ["DIALECT_DDL_CONSTS_RUST", rustDialectDdlConstsBlock(dialects)],
        ["DIALECT_SETUP_DISPATCH_RUST", rustDialectSetupDispatchBlock(dialects)],
        ["DIALECT_SQLITE_URL_HELPER_RUST", sqliteUrl],
    ]);
    const up = await composed("migrate_up.rs", [
        ["DIALECT_USE_IMPORTS_RUST", useForRunners],
        ["DIALECT_RUNNER_FNS_RUST", rustDialectRunnerFnsBlock(dialects)],
        ["DIALECT_UP_DISPATCH_RUST", rustDialectUpDispatchBlock(dialects)],
        ["DIALECT_SQLITE_URL_HELPER_RUST", sqliteUrl],
    ]);
    const down = await composed("migrate_down.rs", [
        ["DIALECT_USE_IMPORTS_RUST", useForRunners],
        ["DIALECT_ROLLBACK_FNS_RUST", rustDialectRollbackFnsBlock(dialects)],
        ["DIALECT_DOWN_DISPATCH_RUST", rustDialectDownDispatchBlock(dialects)],
        ["DIALECT_SQLITE_URL_HELPER_RUST", sqliteUrl],
    ]);
    return [
        content(join(migrateDir, "migrate_setup.rs"), setup),
        content(join(migrateDir, "migrate_up.rs"), up),
        content(join(migrateDir, "migrate_down.rs"), down),
        content(join(migrateDir, "migrate_create.rs"), await runnerTemplate("migrate_create.rs")),
    ];
}
/** Cargo.toml as create-or-update PATCH entries: a seed patch carries the self-complete standalone base (the patch writer writes it only when Cargo.toml is absent), plus the MIGRATE_BIN/MIGRATE_DEPS marked blocks (filled when a combined scaffold's backend_app Cargo.toml is present). The writer picks per on-disk state, so no outDir read is needed. */
async function cargoTomlPatches(migrateDir, dialects) {
    return [
        {
            kind: PATCH,
            filename: "Cargo.toml",
            content: await renderRustMigrateCargoToml(dialects, migrateDir),
        },
        markedEntry("Cargo.toml", "MIGRATE_BIN", rustMigrateBinBlock(migrateDir)),
        markedEntry("Cargo.toml", "MIGRATE_DEPS", buildRustCargoTomlDepsBlock(dialects)),
    ];
}
/** All rust migrate output as CONTENT + PATCH entries. A combined scaffold patches backend_app's Cargo.toml/Dockerfile/.env/entrypoint; a standalone scope's Cargo.toml is created by the seed patch and the sibling patches no-op on the absent files. */
async function rustEntries({ migrateDir, dialects = [], settings, combined, }) {
    const layout = layoutForSettings(settings, "rust");
    const { lane, shared } = layout.migrateDockerCopyPrefixes({ combined });
    // The runner binary must compile the sqlite dispatch arm + sqlx feature so verify's host lane can boot it with `--provider sqlite`; the deployment builders below (gitkeep, .env) stay on the configured production dialects.
    const runnerDialects = withSqliteDialect(dialects);
    return [
        ...(await cargoTomlPatches(migrateDir, runnerDialects)),
        ...(await runnerFileEntries(migrateDir, runnerDialects)),
        ...gitkeepEntries(dialects, settings),
        ...dockerfileCopyPatches(rustMigrateCopyContent(migrateDir, lane, shared), rustMigrateRuntimeCopyContent(shared)),
        entrypointPatch("rust", migrateDir, layout),
        ...dbFilePatches(dialects),
    ];
}
export const migrateRust = {
    language: "rust",
    emit: rustEntries,
};
export const emit = makeMigrateEmit(rustEntries);
export const flags = [MIGRATE_DIR_FLAG, COMBINED_FLAG];
export const entriesNative = true;
export const pinProjectRoot = true;
export const rustModuleWiring = false;
