#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static CALL_CG4RS_LOCK: Mutex<()> = Mutex::new(());

pub fn unique_output_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

pub fn run_call_cg4rs_with_args(manifest_path: &Path, output_dir: &Path, extra_args: &[&str]) {
    let _guard = CALL_CG4RS_LOCK.lock().expect("call-cg4rs lock poisoned");
    let call_cg4rs = PathBuf::from(env!("CARGO_BIN_EXE_call-cg4rs"));
    let bins_dir = call_cg4rs
        .parent()
        .expect("call-cg4rs binary should have a parent directory");
    let current_path = std::env::var_os("PATH").expect("PATH should be set");
    let prefixed_path =
        std::env::join_paths(std::iter::once(bins_dir.to_path_buf()).chain(std::env::split_paths(&current_path)))
            .expect("failed to construct PATH");

    fs::create_dir_all(output_dir).expect("failed to create test output dir");

    let status = Command::new(&call_cg4rs)
        .env("PATH", prefixed_path)
        .args([
            "--manifest-path",
            manifest_path.to_str().expect("manifest path is not valid utf-8"),
            "--output-dir",
            output_dir.to_str().expect("output dir is not valid utf-8"),
        ])
        .args(extra_args)
        .status()
        .expect("failed to run call-cg4rs");

    assert!(status.success(), "call-cg4rs should exit successfully");
}

pub fn run_call_cg4rs(manifest_path: &Path, output_dir: &Path, find_callers: &str) {
    run_call_cg4rs_with_args(
        manifest_path,
        output_dir,
        &["--find-callers", find_callers, "--json-output"],
    );
}

pub fn read_json(path: &Path) -> Value {
    let content = fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse {} as json: {e}", path.display()))
}

pub fn read_callers_json(output_dir: &Path, target_name: &str) -> Value {
    read_json(&output_dir.join(format!("callers-{target_name}.json")))
}

pub fn read_public_exposure_json(output_dir: &Path, crate_name: &str) -> Value {
    read_json(&output_dir.join(format!("{crate_name}-public-exposure.json")))
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path(relative_path: &str) -> PathBuf {
    repo_root().join(relative_path)
}

pub struct SharedOutput {
    cell: OnceLock<PathBuf>,
}

impl SharedOutput {
    pub const fn new() -> Self {
        Self { cell: OnceLock::new() }
    }

    pub fn get_or_init<F>(&'static self, init: F) -> &'static PathBuf
    where
        F: FnOnce() -> PathBuf,
    {
        self.cell.get_or_init(init)
    }
}
