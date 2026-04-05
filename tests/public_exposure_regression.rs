mod common;

use common::{
    manifest_path, read_json, read_public_exposure_json, run_call_cg4rs, run_call_cg4rs_with_args, unique_output_dir,
};

#[test]
fn public_exposure_output_is_emitted_even_without_targets() {
    let manifest_path = manifest_path("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-public-exposure-empty");

    run_call_cg4rs_with_args(&manifest_path, &output_dir, &["--json-output"]);

    let public_exposure = read_public_exposure_json(&output_dir, "test1");

    assert_eq!(public_exposure["crate_name"].as_str(), Some("test1"));
    assert_eq!(public_exposure["public_exposure"]["total_targets"].as_u64(), Some(0));
    assert_eq!(
        public_exposure["public_exposure"]["public_reachable_targets"].as_u64(),
        Some(0)
    );
    assert_eq!(
        public_exposure["public_exposure_details"]
            .as_array()
            .expect("public_exposure_details should be an array")
            .len(),
        0
    );

    let callgraph = read_json(&output_dir.join("callgraph.json"));
    assert!(
        !callgraph
            .as_array()
            .expect("callgraph.json should be an array of caller-callee records")
            .is_empty(),
        "callgraph.json should still be produced alongside public exposure output"
    );
}

#[test]
fn public_exposure_tracks_reachable_targets_when_callers_are_requested() {
    let manifest_path = manifest_path("testdata/test1/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-public-exposure-targeted");

    run_call_cg4rs(&manifest_path, &output_dir, "fn_trait_example::inc");

    let public_exposure = read_public_exposure_json(&output_dir, "test1");

    assert!(
        public_exposure["public_exposure"]["total_targets"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "targeted analysis should collect at least one target"
    );
    assert!(
        public_exposure["public_exposure"]["public_reachable_targets"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "requested target should be reachable from at least one public entry"
    );
    assert!(
        !public_exposure["public_exposure_details"]
            .as_array()
            .expect("public_exposure_details should be an array")
            .is_empty(),
        "targeted public exposure analysis should emit at least one witness entry"
    );
}
