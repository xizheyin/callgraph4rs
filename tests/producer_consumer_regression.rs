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
fn shallow_producer_consumer_summaries_resolve_returned_callables() {
    let manifest_path = manifest_path("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-producer-consumer");

    run_call_cg4rs(&manifest_path, &output_dir, "plus_hundred,minus_hundred");

    let plus_hundred = read_callers_json(&output_dir, "plus_hundred");
    let minus_hundred = read_callers_json(&output_dir, "minus_hundred");

    assert!(
        has_fnptr_path_from(&plus_hundred, "fn_pointer_example::test7_bonus"),
        "make_bonus_op should let test7_bonus reach plus_hundred through a returned function pointer"
    );
    assert!(
        has_fnptr_path_from(&minus_hundred, "fn_pointer_example::test8_passthrough"),
        "passthrough_op should let test8_passthrough reach minus_hundred through a returned function pointer"
    );
}
