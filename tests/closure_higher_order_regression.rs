mod common;

use common::{SharedOutput, manifest_path, read_callers_json, run_call_cg4rs, unique_output_dir};
use serde_json::Value;

static TEST1_OUTPUT_DIR: SharedOutput = SharedOutput::new();

fn analyzed_test1_output() -> &'static std::path::PathBuf {
    TEST1_OUTPUT_DIR.get_or_init(|| {
        let manifest_path = manifest_path("testdata/test1/Cargo.toml");
        let output_dir = unique_output_dir("cg4rs-closure-higher-order");
        run_call_cg4rs(
            &manifest_path,
            &output_dir,
            "fn_trait_example::inc,fn_trait_example::add_offset",
        );
        output_dir
    })
}

fn read_target_json(target_name: &str) -> Value {
    read_callers_json(analyzed_test1_output(), target_name)
}

fn caller_entry<'a>(value: &'a Value, caller_path: &str) -> Option<&'a Value> {
    value["callers"]
        .as_array()
        .expect("callers should be an array")
        .iter()
        .find(|entry| entry["path"].as_str() == Some(caller_path))
}

#[test]
fn closure_passed_as_argument_is_resolved_end_to_end() {
    let inc = read_target_json("fn_trait_example::inc");

    let higher_order_path = caller_entry(&inc, "fn_trait_example::xxtest5")
        .expect("xxtest5 should reach inc through a closure passed into call_with_fn");
    assert_eq!(higher_order_path["path_dyn_edges"].as_u64(), Some(0));
    assert_eq!(higher_order_path["path_fnptr_edges"].as_u64(), Some(0));
    assert!(
        higher_order_path["path_len"].as_u64().unwrap_or(0) >= 3,
        "higher-order closure path should include the helper and closure body"
    );
}

#[test]
fn captured_and_nested_closures_are_traced() {
    let add_offset = read_target_json("fn_trait_example::add_offset");

    let captured_path = caller_entry(&add_offset, "fn_trait_example::xxtest6")
        .expect("xxtest6 should reach add_offset through a capturing closure");
    assert_eq!(captured_path["path_dyn_edges"].as_u64(), Some(0));
    assert_eq!(captured_path["path_fnptr_edges"].as_u64(), Some(0));

    let nested_path = caller_entry(&add_offset, "fn_trait_example::xxtest7")
        .expect("xxtest7 should reach add_offset through call_nested_fn and a capturing closure");
    assert_eq!(nested_path["path_dyn_edges"].as_u64(), Some(0));
    assert_eq!(nested_path["path_fnptr_edges"].as_u64(), Some(0));
    assert!(
        nested_path["path_len"].as_u64().unwrap_or(0) >= 4,
        "nested closure path should include the wrapper helper"
    );
}
