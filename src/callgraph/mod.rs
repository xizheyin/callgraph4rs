mod analysis;
mod fmt;
mod function;
mod types;
mod utils;

use analysis::perform_mono_analysis;
use fmt::{output_call_graph_result, output_callers_result};
use function::FunctionInstance;
use std::collections::VecDeque;
use types::CallSite;

// Main entry point for callgraph analysis
pub fn analyze_crate<'tcx>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    options: &crate::args::CGArgs,
) -> CallGraph<'tcx> {
    // Collect all generic instances in the crate
    let instances = function::collect_generic_instances(tcx);

    // Perform monomorphization analysis
    let call_graph = perform_mono_analysis(tcx, instances, options);

    // Handle find_callers_of and find_callers_by_hash options (mutually exclusive)
    match (&options.find_callers, &options.find_callers_by_hash) {
        (Some(target_path), None) => {
            // Only find_callers_of is specified
            tracing::info!("Finding callers of function: {}", target_path);
            if let Some(callers) = call_graph.find_callers_by_path(tcx, target_path) {
                output_callers_result(&call_graph, tcx, target_path, callers, options, "callers");
            }
        }
        (None, Some(target_hash)) => {
            // Only find_callers_by_hash is specified
            tracing::info!("Finding callers of function with hash: {}", target_hash);
            if let Some(callers) = call_graph.find_callers_by_hash(tcx, target_hash) {
                let target_display = format!("function with hash: {}", target_hash);
                output_callers_result(
                    &call_graph,
                    tcx,
                    &target_display,
                    callers,
                    options,
                    "callers_by_hash",
                );
            }
        }
        (Some(_), Some(_)) => {
            // Both options are specified - warn user and prioritize path-based search
            tracing::error!(
                "Both --find-callers-of and --find-callers-by-hash options are specified.\
                    Using -h to show help."
            );
            std::process::exit(1);
        }
        (None, None) => {
            // Neither option is specified - do nothing
        }
    }

    // If JSON output is requested, write call graph as JSON
    if options.json_output {
        let json_output = call_graph.format_call_graph_as_json(tcx);

        // Output to file
        let output_path = options
            .output_dir
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("./target"))
            .join("callgraph.json");

        if let Err(e) = std::fs::write(&output_path, json_output) {
            tracing::error!("Failed to write JSON call graph to file: {:?}", e);
        } else {
            tracing::info!("JSON call graph written to: {:?}", output_path);
        }
    }

    output_call_graph_result(&call_graph, tcx, options);

    call_graph
}

pub(crate) struct CallGraph<'tcx> {
    _all_generic_instances: Vec<FunctionInstance<'tcx>>,
    instances: VecDeque<FunctionInstance<'tcx>>,
    pub call_sites: Vec<CallSite<'tcx>>,
    without_args: bool,
}

impl<'tcx> CallGraph<'tcx> {
    fn new(all_generic_instances: Vec<FunctionInstance<'tcx>>, without_args: bool) -> Self {
        Self {
            _all_generic_instances: all_generic_instances.clone(),
            instances: all_generic_instances.into_iter().collect(),
            call_sites: Vec::new(),
            without_args,
        }
    }
}
