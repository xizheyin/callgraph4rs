mod common;

use common::{manifest_path, read_callers_json, run_call_cg4rs, unique_output_dir};
use serde_json::Value;

fn has_fnptr_path_from(value: &Value, caller: &str) -> bool {
    value["callers"]
        .as_array()
        .expect("callers should be an array")
        .iter()
        .any(|entry| entry["path"].as_str() == Some(caller) && entry["path_fnptr_edges"].as_u64().unwrap_or(0) >= 1)
}

#[test]
fn generic_fnptr_signature_fallback_recovers_common_pointer_shapes() {
    let manifest_path = manifest_path("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-fnptr-signature");

    run_call_cg4rs(
        &manifest_path,
        &output_dir,
        "fn_pointer_example::generic_slot_reader,fn_pointer_example::generic_ptr_sink",
    );

    let generic_slot_reader = read_callers_json(&output_dir, "fn_pointer_example::generic_slot_reader");
    let generic_ptr_sink = read_callers_json(&output_dir, "fn_pointer_example::generic_ptr_sink");

    assert!(
        has_fnptr_path_from(&generic_slot_reader, "fn_pointer_example::invoke_slot_reader"),
        "invoke_slot_reader should reach generic_slot_reader through a function-pointer edge"
    );
    assert!(
        has_fnptr_path_from(&generic_ptr_sink, "fn_pointer_example::invoke_ptr_sink"),
        "invoke_ptr_sink should reach generic_ptr_sink through a function-pointer edge"
    );
}
