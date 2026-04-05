mod common;

use common::{manifest_path, read_callers_json, run_call_cg4rs, unique_output_dir};

#[test]
fn async_stream_fixture_produces_stable_callers_output() {
    let manifest_path = manifest_path("testdata/test2/Cargo.toml");
    let output_dir = unique_output_dir("cg4rs-async-stream");

    // `CounterStream::new` is a stable smoke target in the async fixture:
    // it is called from the async main body and reliably appears in callers JSON.
    run_call_cg4rs(&manifest_path, &output_dir, "CounterStream::new");

    let ctor = read_callers_json(&output_dir, "CounterStream::new");

    assert_eq!(ctor["target"].as_str(), Some("CounterStream::new"));
    assert_eq!(ctor["total_callers"].as_u64(), Some(2));
    assert!(
        ctor["callers"]
            .as_array()
            .expect("callers should be an array")
            .iter()
            .any(|entry| entry["path"].as_str() == Some("main::{closure#0}")),
        "CounterStream::new should be directly reachable from the async main closure"
    );
    assert!(
        ctor["callers"]
            .as_array()
            .expect("callers should be an array")
            .iter()
            .any(|entry| entry["path"].as_str() == Some("main")),
        "CounterStream::new should also be reachable from the top-level main entry"
    );
    assert_eq!(
        ctor["reachability_summary"]["call_kind_counts"]["Direct"].as_u64(),
        Some(1)
    );
}
