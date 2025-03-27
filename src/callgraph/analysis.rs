use std::collections::{HashMap, HashSet};

use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir,
    ty::{self, Instance, TyCtxt, TypeFoldable, TypingEnv},
};

use crate::constraint_utils::{self, BlockPath};

use super::{function::FunctionInstance, CallGraph};

pub(crate) fn trivial_resolve(tcx: ty::TyCtxt<'_>, def_id: DefId) -> Option<FunctionInstance<'_>> {
    let ty = tcx.type_of(def_id).skip_binder();
    if let ty::TyKind::FnDef(def_id, args) = ty.kind() {
        let instance =
            ty::Instance::try_resolve(tcx, TypingEnv::post_analysis(tcx, def_id), *def_id, args);
        if let Ok(Some(instance)) = instance {
            Some(FunctionInstance::new_instance(instance))
        } else {
            None
        }
    } else {
        None
    }
}

pub(crate) fn perform_mono_analysis<'tcx>(
    tcx: ty::TyCtxt<'tcx>,
    instances: Vec<FunctionInstance<'tcx>>,
    options: &crate::args::CGArgs,
) -> CallGraph<'tcx> {
    let mut call_graph = CallGraph::new(instances, options.without_args);
    let mut visited = HashSet::new();

    while let Some(instance) = call_graph.instances.pop_front() {
        if visited.contains(&instance) {
            continue;
        }
        visited.insert(instance);

        let call_sites = instance.collect_callsites(tcx);
        for call_site in call_sites {
            call_graph.instances.push_back(call_site.callee());
            call_graph.call_sites.push(call_site);
        }
    }

    // Deduplicate call sites if deduplication is not disabled
    if !options.no_dedup {
        tracing::info!("Deduplication enabled - removing duplicate call sites");
        call_graph.deduplicate_call_sites();
    } else {
        tracing::info!("Deduplication disabled - keeping all call sites");
    }

    // If a function to find is specified, find its callers
    if let Some(target_path) = &options.find_callers_of {
        tracing::info!("Finding callers of function: {}", target_path);
        if let Some(callers) = call_graph.find_callers(tcx, target_path) {
            // Get output directory
            let output_dir = options
                .output_dir
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from("./target"));

            // Determine output format (text or JSON)
            if options.json_output {
                // Generate JSON output for callers
                let callers_json = call_graph.format_callers_as_json(tcx, target_path, callers);

                // Output to JSON file
                let json_output_path = output_dir.join("callers.json");

                if let Err(e) = std::fs::write(&json_output_path, callers_json) {
                    tracing::error!("Failed to write callers to JSON file: {:?}", e);
                } else {
                    tracing::info!("Callers JSON output written to: {:?}", json_output_path);
                }
            } else {
                // Generate text output for callers (original behavior)
                let callers_output = call_graph.format_callers(tcx, target_path, callers);

                // Output to text file
                let output_path = output_dir.join("callers.txt");

                if let Err(e) = std::fs::write(&output_path, callers_output) {
                    tracing::error!("Failed to write callers to file: {:?}", e);
                } else {
                    tracing::info!("Callers output written to: {:?}", output_path);
                }
            }
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

    call_graph
}

pub fn monomorphize<'tcx, T>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    instance: Instance<'tcx>,
    value: T,
) -> Result<T, ty::normalize_erasing_regions::NormalizationError<'tcx>>
where
    T: TypeFoldable<TyCtxt<'tcx>>,
{
    instance.try_instantiate_mir_and_normalize_erasing_regions(
        tcx,
        typing_env,
        ty::EarlyBinder::bind(value),
    )
}

pub(crate) fn get_constraints(
    tcx: ty::TyCtxt,
    def_id: DefId,
) -> HashMap<mir::BasicBlock, BlockPath> {
    let mir = tcx.optimized_mir(def_id);
    constraint_utils::compute_shortest_paths(mir)
}
