mod common;

use common::{manifest_path, read_callers_json, run_call_cg4rs, unique_output_dir};
use serde_json::Value;

fn has_dyn_path_from(value: &Value, caller: &str) -> bool {
    value["callers"]
        .as_array()
        .expect("callers should be an array")
        .iter()
        .any(|entry| entry["path"].as_str() == Some(caller) && entry["path_dyn_edges"].as_u64().unwrap_or(0) >= 1)
}

#[test]
fn boxed_dyn_fn_wrappers_resolve_to_concrete_targets() {
    let manifest_path = manifest_path("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-dyn-fn-wrapper");

    run_call_cg4rs(
        &manifest_path,
        &output_dir,
        "fn_trait_example::inc,fn_trait_example::xxtest4::{closure#0}",
    );

    let inc = read_callers_json(&output_dir, "fn_trait_example::inc");
    let boxed_closure = read_callers_json(&output_dir, "fn_trait_example::xxtest4::{closure#0}");

    assert!(
        has_dyn_path_from(&inc, "fn_trait_example::xxtest3"),
        "boxed dyn FnMut function item should let xxtest3 reach inc through a dyn edge"
    );
    assert!(
        has_dyn_path_from(&boxed_closure, "fn_trait_example::xxtest4"),
        "boxed dyn FnMut closure should let xxtest4 reach its concrete closure through a dyn edge"
    );
}
