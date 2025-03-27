mod analysis;
mod fmt;
mod function;
mod types;
mod utils;

use analysis::perform_mono_analysis;
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
    perform_mono_analysis(tcx, instances, options)
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
