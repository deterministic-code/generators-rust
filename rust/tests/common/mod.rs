#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust crate has parent")
        .to_path_buf()
}

/// Run `emit.ts --step <step> --language shared` for a sample's `deterministic/` dir into `output`, panicking on a non-zero exit. Shared by every functional test that needs emitted SQL on disk.
pub fn emit_step(step: &str, deterministic_dir: &Path, output: &Path) {
    let codegen = repo_root().join("scripts").join("codegen").join("emit.ts");
    let status = Command::new("node")
        .arg(&codegen)
        .args(["--step", step, "--language", "shared"])
        .args(["--deterministic-dir", deterministic_dir.to_str().unwrap()])
        .args(["--output", output.to_str().unwrap()])
        .status()
        .unwrap_or_else(|e| panic!("spawn emit.ts --step {step}: {e}"));
    assert!(status.success(), "emit.ts --step {step} failed");
}

/// migrate-setup then migrate-up for `provider` against `connection`, applying every migration under `migrations_dir` in order.
fn run_migrate_chain(provider: &str, connection: &str, migrations_dir: &Path) {
    let setup = option_env!("CARGO_BIN_EXE_migrate-setup")
        .expect("migrate-setup bin is not built for this crate");
    let up = option_env!("CARGO_BIN_EXE_migrate-up")
        .expect("migrate-up bin is not built for this crate");
    let setup_status = Command::new(setup)
        .args(["--provider", provider])
        .args(["--connection", connection])
        .status()
        .expect("spawn migrate-setup");
    assert!(setup_status.success(), "migrate-setup failed");

    let up_status = Command::new(up)
        .args(["--provider", provider])
        .args(["--connection", connection])
        .args(["--migrations-path", migrations_dir.to_str().unwrap()])
        .status()
        .expect("spawn migrate-up");
    assert!(up_status.success(), "migrate-up failed");
}

/// Emit both the CREATE-TABLE (`sql`) and `stored_procedures` steps for a sample, then run migrate-setup + migrate-up for `provider` (postgres/mysql) against an already-running `connection` — so a functional test exercises the real emitted `0001_initial` + `0002_stored_procedures` chain through the migrate-up binary. The returned `TempDir` owns the emitted SQL tree.
pub fn migrate_provider_for(deterministic_dir: &Path, provider: &str, connection: &str) -> TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let migrations_root = tmp.path().join("sql");
    emit_step("sql", deterministic_dir, &migrations_root);
    emit_step("stored_procedures", deterministic_dir, &migrations_root);
    let migrations_dir = migrations_root.join(provider).join("migrations");
    run_migrate_chain(provider, connection, &migrations_dir);
    tmp
}
