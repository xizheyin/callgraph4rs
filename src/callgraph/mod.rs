mod analysis;
mod controlflow;
mod fmt;
mod function;
mod origin;
mod public_exposure;
mod resolution;
mod summary;
mod types;
mod utils;

use analysis::perform_mono_analysis;
use fmt::{output_call_graph_result, output_callers_result};
use function::FunctionInstance;
use types::CallGraph;

// Main entry point for callgraph analysis
pub fn analyze_crate<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, args: &crate::args::CGArgs) -> CallGraph<'tcx> {
    // Collect all generic instances in the crate
    let instances: Vec<FunctionInstance<'tcx>> =
        crate::timer::measure("0collect_local_instances", || function::collect_local_instances(tcx));

    // Perform monomorphization analysis
    let call_graph: CallGraph<'tcx> =
        crate::timer::measure("1perform_mono_analysis", || perform_mono_analysis(tcx, instances, args));

    // Handle find_callers_of
    crate::timer::measure("2output_find_callers_results", || {
        for target_path in &args.find_callers {
            tracing::debug!("Finding callers of function: {}", target_path);
            let callers_with_constraints = call_graph.find_callers_by_path(tcx, target_path);
            crate::timer::measure("output_callers_result", || {
                output_callers_result(
                    &call_graph,
                    tcx,
                    target_path,
                    callers_with_constraints,
                    args,
                    &format!("callers-{target_path}"),
                )
            });
        }
    });

    crate::timer::measure("output_call_graph_result", || {
        output_call_graph_result(&call_graph, tcx, args)
    });

    // Perform public exposure analysis
    crate::timer::measure("public_exposure_analysis", || {
        public_exposure::analyze_public_exposure(&call_graph, tcx, args);
    });

    call_graph
}
