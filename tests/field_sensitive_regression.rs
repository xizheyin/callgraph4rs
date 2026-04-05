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
fn struct_field_fnptr_calls_are_traced_precisely() {
    let manifest_path = manifest_path("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-field-sensitive");

    run_call_cg4rs(&manifest_path, &output_dir, "add_one,times_two");

    let add_one = read_callers_json(&output_dir, "add_one");
    let times_two = read_callers_json(&output_dir, "times_two");

    assert!(
        has_fnptr_path_from(&add_one, "fn_pointer_example::OpHolder::apply"),
        "OpHolder::apply should reach add_one through its function-pointer field"
    );
    assert!(
        has_fnptr_path_from(&times_two, "fn_pointer_example::OpHolder::apply"),
        "OpHolder::apply should reach times_two through its function-pointer field"
    );
}
