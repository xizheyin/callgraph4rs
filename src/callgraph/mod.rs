mod analysis;
mod fmt;
mod function;
mod types;
mod utils;

use crate::timer::Timer;
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
    let instances = crate::timer::measure("collect_generic_instances", || {
        function::collect_generic_instances(tcx)
    });

    // Log the number of instances found
    tracing::info!("Collected {} generic instances", instances.len());

    // Perform monomorphization analysis
    let call_graph = crate::timer::measure("perform_mono_analysis", || {
        perform_mono_analysis(tcx, instances, options)
    });

    // Log call site count
    tracing::info!("Found {} call sites", call_graph.call_sites.len());

    // Handle find_callers_of and find_callers_by_hash options (mutually exclusive)
    match (&options.find_callers, &options.find_callers_by_hash) {
        (Some(target_path), None) => {
            // Only find_callers_of is specified
            tracing::info!("Finding callers of function: {}", target_path);
            Timer::start("find_callers_by_path");
            if let Some(callers_with_constraints) =
                call_graph.find_callers_by_path(tcx, target_path)
            {
                Timer::stop("find_callers_by_path");
                crate::timer::measure("output_callers_result", || {
                    output_callers_result(
                        &call_graph,
                        tcx,
                        target_path,
                        callers_with_constraints,
                        options,
                        "callers",
                    )
                });
            } else {
                Timer::stop("find_callers_by_path");
            }
        }
        (None, Some(target_hash)) => {
            // Only find_callers_by_hash is specified
            tracing::info!("Finding callers of function with hash: {}", target_hash);
            Timer::start("find_callers_by_hash");
            if let Some(callers_with_constraints) =
                call_graph.find_callers_by_hash(tcx, target_hash)
            {
                Timer::stop("find_callers_by_hash");
                let target_display = format!("function with hash: {}", target_hash);
                crate::timer::measure("output_callers_result", || {
                    output_callers_result(
                        &call_graph,
                        tcx,
                        &target_display,
                        callers_with_constraints,
                        options,
                        "callers_by_hash",
                    )
                });
            } else {
                Timer::stop("find_callers_by_hash");
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

    crate::timer::measure("output_call_graph_result", || {
        output_call_graph_result(&call_graph, tcx, options)
    });

    call_graph
}

pub(crate) struct CallGraph<'tcx> {
    instances: VecDeque<FunctionInstance<'tcx>>,
    pub call_sites: Vec<CallSite<'tcx>>,
    without_args: bool,
}

impl<'tcx> CallGraph<'tcx> {
    fn new(all_generic_instances: Vec<FunctionInstance<'tcx>>, without_args: bool) -> Self {
        Self {
            instances: all_generic_instances.into_iter().collect(),
            call_sites: Vec::new(),
            without_args,
        }
    }
}
