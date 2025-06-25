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

// Main entry point for callgraph analysis
pub fn analyze_crate<'tcx>(
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    args: &crate::args::CGArgs,
) -> CallGraph<'tcx> {
    // Collect all generic instances in the crate
    let instances: Vec<FunctionInstance<'tcx>> =
        crate::timer::measure("collect_generic_instances", || {
            function::collect_generic_instances(tcx)
        });

    // Perform monomorphization analysis
    let call_graph: CallGraph<'tcx> = crate::timer::measure("perform_mono_analysis", || {
        perform_mono_analysis(tcx, instances, args)
    });

    // Handle find_callers_of
    crate::timer::measure("output_find_callers_results", || {
        output_find_callers_results(&call_graph, tcx, args)
    });

    crate::timer::measure("output_call_graph_result", || {
        output_call_graph_result(&call_graph, tcx, args)
    });

    call_graph
}

/// Handle find_callers_of
fn output_find_callers_results<'tcx>(
    call_graph: &CallGraph<'tcx>,
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    args: &crate::args::CGArgs,
) {
    if args.find_callers.is_empty() {
        return;
    }

    for target_path in &args.find_callers {
        tracing::info!("Finding callers of function: {}", target_path);
        if let Some(callers_with_constraints) = call_graph.find_callers_by_path(tcx, target_path) {
            crate::timer::measure("output_callers_result", || {
                output_callers_result(
                    call_graph,
                    tcx,
                    target_path,
                    callers_with_constraints,
                    args,
                    &format!("callers-{}", target_path),
                )
            });
        }
    }
}
