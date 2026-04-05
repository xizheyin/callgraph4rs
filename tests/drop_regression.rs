mod common;

use common::{SharedOutput, manifest_path, read_json, run_call_cg4rs, unique_output_dir};
use serde_json::Value;

static TEST1_OUTPUT_DIR: SharedOutput = SharedOutput::new();

fn analyzed_test1_output() -> &'static std::path::PathBuf {
    TEST1_OUTPUT_DIR.get_or_init(|| {
        let manifest_path = manifest_path("testdata/test1/Cargo.toml");
        let output_dir = unique_output_dir("cg4rs-drop");
        run_call_cg4rs(&manifest_path, &output_dir, "drop_sink");
        output_dir
    })
}

fn callgraph_entries() -> Vec<Value> {
    read_json(&analyzed_test1_output().join("callgraph.json"))
        .as_array()
        .expect("callgraph.json should be an array")
        .to_vec()
}

#[test]
fn explicit_drop_terminator_adds_drop_edge_to_drop_impl() {
    let entries = callgraph_entries();

    let trigger_entry = entries
        .iter()
        .find(|entry| entry["caller"]["path"].as_str() == Some("trigger_scope_drop"))
        .expect("trigger_scope_drop should appear in callgraph.json");

    let callees = trigger_entry["callee"]
        .as_array()
        .expect("callee list should be an array");

    assert!(
        callees
            .iter()
            .any(|callee| callee["path"].as_str() == Some("<DropTracer as std::ops::Drop>::drop")),
        "trigger_scope_drop should reach the destructor through a Drop edge"
    );
}
