mod common;

use common::{SharedOutput, manifest_path, read_callers_json, run_call_cg4rs, unique_output_dir};
use serde_json::Value;
static TEST1_OUTPUT_DIR: SharedOutput = SharedOutput::new();

fn analyzed_test1_output() -> &'static std::path::PathBuf {
    TEST1_OUTPUT_DIR.get_or_init(|| {
        let manifest_path = manifest_path("testdata/test1/Cargo.toml");
        let output_dir = unique_output_dir("cg4rs-testdata-regressions");
        run_call_cg4rs(
            &manifest_path,
            &output_dir,
            "dyn_example::Signal::sample,dyn_example::Signal::name,fn_trait_example::inc,DataStore::total_value,log_calculation",
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

fn has_caller_path(value: &Value, caller_path: &str) -> bool {
    caller_entry(value, caller_path).is_some()
}

fn caller_paths(value: &Value) -> Vec<String> {
    value["callers"]
        .as_array()
        .expect("callers should be an array")
        .iter()
        .filter_map(|entry| entry["path"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn dyn_trait_callers_are_reported_for_trait_method_targets() {
    let sample = read_target_json("dyn_example::Signal::sample");
    let name = read_target_json("dyn_example::Signal::name");

    assert_eq!(sample["target"].as_str(), Some("dyn_example::Signal::sample"));
    assert!(has_caller_path(&sample, "dyn_example::process_signal"));
    assert!(has_caller_path(&sample, "dyn_example::analyze_signal"));
    assert!(has_caller_path(&sample, "dyn_example::mix_signals"));
    assert!(has_caller_path(&sample, "dyn_example::share_signal"));

    assert_eq!(name["target"].as_str(), Some("dyn_example::Signal::name"));
    assert!(has_caller_path(&name, "dyn_example::process_signal"));
    assert!(has_caller_path(&name, "dyn_example::analyze_signal"));
    assert!(has_caller_path(&name, "dyn_example::share_signal"));
}

#[test]
fn fn_trait_calls_include_dyn_dispatch_edges() {
    let inc = read_target_json("fn_trait_example::inc");

    let dyn_fn_caller =
        caller_entry(&inc, "fn_trait_example::xxtest1").expect("xxtest1 should call inc through dyn Fn");
    assert_eq!(dyn_fn_caller["path_dyn_edges"].as_u64(), Some(1));
    assert_eq!(dyn_fn_caller["path_fnptr_edges"].as_u64(), Some(0));

    assert!(
        inc["reachability_summary"]["call_kind_counts"]["DynTrait"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "inc should have at least one dyn-trait mediated caller path"
    );
}

#[test]
fn generic_and_closure_callers_are_reported() {
    let total_value = read_target_json("DataStore::total_value");
    let log_calculation = read_target_json("log_calculation");

    assert_eq!(total_value["target"].as_str(), Some("DataStore::total_value"));
    assert!(has_caller_path(
        &total_value,
        "InventoryManager::generate_inventory_report"
    ));
    assert!(
        total_value["reachability_summary"]["target_variants_count"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "base-path matching should capture multiple generic variants"
    );
    assert!(
        total_value["reachability_summary"]["target_unique_def_ids_count"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "total_value should include both generic and concrete variants"
    );

    let log_paths = caller_paths(&log_calculation);
    assert!(
        log_paths.iter().any(|path| path == "DataStore::<T>::total_value"),
        "log_calculation should be reachable from total_value; callers were {log_paths:?}"
    );
    assert!(
        log_paths
            .iter()
            .any(|path| path == "DataStore::<T>::total_discounted_value"),
        "log_calculation should be reachable from total_discounted_value; callers were {log_paths:?}"
    );
}
