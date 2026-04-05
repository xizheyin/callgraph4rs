mod common;

use common::{manifest_path, read_callers_json, run_call_cg4rs, unique_output_dir};

#[test]
fn missing_target_produces_empty_callers_json_instead_of_failing() {
    let manifest_path = manifest_path("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-missing-target");

    run_call_cg4rs(&manifest_path, &output_dir, "definitely_not_a_real_function");

    let missing = read_callers_json(&output_dir, "definitely_not_a_real_function");

    assert_eq!(missing["target"].as_str(), Some("definitely_not_a_real_function"));
    assert_eq!(missing["total_callers"].as_u64(), Some(0));
    assert_eq!(
        missing["callers"].as_array().expect("callers should be an array").len(),
        0
    );
    assert_eq!(
        missing["reachability_summary"]["target_variants_count"].as_u64(),
        Some(0)
    );
    assert_eq!(
        missing["reachability_summary"]["target_unique_def_ids_count"].as_u64(),
        Some(0)
    );
}
