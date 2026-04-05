/// Integration test for resolution module functionality
/// Tests function pointer and dynamic trait resolution
mod common;

use common::{run_call_cg4rs, unique_output_dir};
use std::path::PathBuf;

#[test]
fn test_resolution_module_integration() {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-resolution-test");

    // Run analysis with multiple find-callers targets
    run_call_cg4rs(&manifest_path, &output_dir, "fn_pointer_example::add");

    // Verify output files were created
    assert!(output_dir.join("callers-fn_pointer_example::add.json").exists());
}
