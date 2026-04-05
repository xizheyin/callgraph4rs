mod common;

use common::{manifest_path, read_callers_json, run_call_cg4rs, unique_output_dir};
use serde_json::Value;

fn caller_paths(value: &Value) -> Vec<String> {
    value["callers"]
        .as_array()
        .expect("callers should be an array")
        .iter()
        .filter_map(|entry| entry["path"].as_str().map(str::to_owned))
        .collect()
}

fn has_fnptr_path_from(value: &Value, caller: &str) -> bool {
    value["callers"]
        .as_array()
        .expect("callers should be an array")
        .iter()
        .any(|entry| entry["path"].as_str() == Some(caller) && entry["path_fnptr_edges"].as_u64().unwrap_or(0) >= 1)
}

#[test]
fn cross_block_fnptr_resolution_stays_precise() {
    let manifest_path = manifest_path("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-fnptr-cfg");

    run_call_cg4rs(&manifest_path, &output_dir, "add_one,square,times_two,negate");

    let add_one = read_callers_json(&output_dir, "add_one");
    let square = read_callers_json(&output_dir, "square");
    let times_two = read_callers_json(&output_dir, "times_two");
    let negate = read_callers_json(&output_dir, "negate");

    assert!(
        has_fnptr_path_from(&add_one, "fn_pointer_example::call_after_cfg_join"),
        "call_after_cfg_join should reach add_one through a function-pointer edge"
    );
    assert!(
        has_fnptr_path_from(&square, "fn_pointer_example::call_after_cfg_join"),
        "call_after_cfg_join should reach square through a function-pointer edge"
    );

    let times_two_callers = caller_paths(&times_two);
    assert!(
        !times_two_callers
            .iter()
            .any(|path| path == "fn_pointer_example::call_after_cfg_join"),
        "call_after_cfg_join should not be treated as a caller of times_two; callers were {times_two_callers:?}"
    );

    let negate_callers = caller_paths(&negate);
    assert!(
        !negate_callers
            .iter()
            .any(|path| path == "fn_pointer_example::call_after_cfg_join"),
        "call_after_cfg_join should not be treated as a caller of negate; callers were {negate_callers:?}"
    );
}
