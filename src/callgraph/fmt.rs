use crate::callgraph::CallGraph;
use crate::callgraph::types::PathInfo;
use crate::callgraph::utils::get_crate_version;
use rustc_middle::ty::TyCtxt;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::Path;

use super::function::FunctionInstance;
use super::types::CallSite;

impl<'tcx> CallGraph<'tcx> {
    /// Format the call graph as readable text
    pub(crate) fn format_call_graph(&self, tcx: TyCtxt<'tcx>) -> String {
        let mut result = String::new();

        result.push_str("Call Graph:\n");
        result.push_str("===========\n\n");

        // Organize calls by caller
        let mut calls_by_caller: HashMap<FunctionInstance<'tcx>, Vec<&CallSite<'tcx>>> = HashMap::new();

        for call_site in &self.call_sites {
            calls_by_caller.entry(call_site.caller()).or_default().push(call_site);
        }

        // Sort callers to get consistent output
        let mut callers: Vec<FunctionInstance<'tcx>> = calls_by_caller.keys().cloned().collect();
        callers.sort_by_key(|caller| format!("{caller:?}"));

        for caller in callers {
            // Get caller name
            let caller_name = caller.full_path(tcx, self.without_args);
            result.push_str(&format!("Function: {caller_name}\n"));

            // Get all calls from this caller
            if let Some(calls) = calls_by_caller.get(&caller) {
                // Sort by callee and constraint count
                let mut sorted_calls = calls.clone();
                sorted_calls.sort_by(|a, b| {
                    let a_name = a.callee().full_path(tcx, self.without_args);
                    let b_name = b.callee().full_path(tcx, self.without_args);
                    a_name
                        .cmp(&b_name)
                        .then_with(|| a.constraint_count().cmp(&b.constraint_count()))
                });

                // Output call information
                for call in sorted_calls {
                    let callee_name = call.callee().full_path(tcx, self.without_args);
                    result.push_str(&format!(
                        "  -> {} [constraint: {}]\n",
                        callee_name,
                        call.constraint_count()
                    ));
                }

                result.push('\n');
            }
        }

        result
    }

    /// Format the call graph as JSON
    pub(crate) fn format_call_graph_as_json(&self, tcx: TyCtxt<'tcx>) -> String {
        // Create a map to organize calls by caller
        let mut calls_by_caller: HashMap<FunctionInstance<'tcx>, Vec<&CallSite<'tcx>>> = HashMap::new();

        for call_site in &self.call_sites {
            calls_by_caller.entry(call_site.caller()).or_default().push(call_site);
        }

        // Sort callers to get consistent output
        let mut callers: Vec<FunctionInstance<'tcx>> = calls_by_caller.keys().cloned().collect();
        callers.sort_by_key(|caller| format!("{caller:?}"));

        // Create the JSON array to hold all entries
        let mut json_entries = Vec::new();

        for caller in callers {
            // Get caller name and information
            let caller_name = caller.full_path(tcx, self.without_args);
            let caller_def_id = caller.def_id();
            let caller_path = tcx.def_path_str(caller_def_id);

            // Get all calls from this caller
            if let Some(calls) = calls_by_caller.get(&caller) {
                // Sort by callee for consistent output
                let mut sorted_calls = calls.clone();
                sorted_calls.sort_by(|a, b| {
                    let a_name = a.callee().full_path(tcx, self.without_args);
                    let b_name = b.callee().full_path(tcx, self.without_args);
                    a_name
                        .cmp(&b_name)
                        .then_with(|| a.constraint_count().cmp(&b.constraint_count()))
                });

                // Create an array of callee objects
                let mut callees = Vec::new();
                for call in sorted_calls {
                    let callee_name = call.callee().full_path(tcx, self.without_args);
                    let callee_def_id = call.callee().def_id();
                    let callee_path = tcx.def_path_str(callee_def_id);

                    // Get actual version information for this callee
                    let version = get_crate_version(tcx, callee_def_id);

                    // Add callee entry
                    callees.push(json!({
                        "name": callee_name,
                        "version": version,
                        "path": callee_path,
                        "constraint_depth": call.constraint_count(),
                        "package_num": call.package_num()
                    }));
                }

                // Get actual version information for caller
                let caller_version = get_crate_version(tcx, caller_def_id);

                // Calculate the maximum constraint depth
                let max_constraint_depth = calls.iter().map(|c| c.constraint_count()).max().unwrap_or(0);

                // Create the full entry with caller and callees
                let entry = json!({
                    "caller": {
                        "name": caller_name,
                        "version": caller_version,
                        "path": caller_path,
                        "constraint_depth": max_constraint_depth,
                    },
                    "callee": callees
                });

                json_entries.push(entry);
            }
        }

        // Format the entire array as a pretty-printed JSON string
        serde_json::to_string_pretty(&json_entries).unwrap_or_else(|_| "[]".to_string())
    }

    /// Format caller information as readable text
    pub(crate) fn format_callers(&self, tcx: TyCtxt<'tcx>, target_path: &str, callers: Vec<PathInfo<'tcx>>) -> String {
        let mut result = String::new();

        result.push_str(&format!("Callers of functions matching '{target_path}':\n"));
        result.push_str("==================================\n\n");

        // Sort callers to get consistent output - first by constraint count, then by name
        let mut sorted_callers = callers;
        sorted_callers.sort();

        for PathInfo {
            caller,
            constraints,
            package_num,
            package_num_unique,
            path_len,
            ..
        } in &sorted_callers
        {
            let caller_name = caller.full_path(tcx, self.without_args);
            result.push_str(&format!(
                "- {caller_name} [path constraints: {constraints}, package num: {package_num}, package num unique: {package_num_unique}, path len: {path_len}]\n"
            ));
        }

        result.push_str(&format!("\nTotal: {} callers found\n", sorted_callers.len()));
        result
    }

    /// Format caller information as JSON
    pub(crate) fn format_callers_as_json(
        &self,
        tcx: TyCtxt<'tcx>,
        target_path: &str,
        callers: Vec<PathInfo<'tcx>>,
    ) -> String {
        // Sort callers to get consistent output - first by constraint count, then by name
        let mut sorted_callers = callers;
        sorted_callers.sort();

        // Create array for caller information
        let mut caller_entries = Vec::new();

        for PathInfo {
            caller,
            constraints,
            package_num,
            package_num_unique,
            path_len,
            dyn_edges,
            fnptr_edges,
            generic_args_len_sum,
        } in &sorted_callers
        {
            let caller_name = caller.full_path(tcx, self.without_args);
            let caller_def_id = caller.def_id();
            let caller_path = tcx.def_path_str(caller_def_id);

            // Get version information
            let version = get_crate_version(tcx, caller_def_id);

            // Add caller entry
            caller_entries.push(json!({
                "name": caller_name,
                "version": version,
                "path": caller_path,
                "path_constraints": constraints,
                "path_package_num": package_num,
                "path_package_num_unique": package_num_unique,
                "path_len": path_len,
                "path_dyn_edges": dyn_edges,
                "path_fnptr_edges": fnptr_edges,
                "path_generic_args_len_sum": generic_args_len_sum
            }));
        }

        // Compute aggregated path-level RQ3 metrics
        let mut dyn_ratio_sum = 0.0;
        let mut gen_args_per_edge_sum = 0.0;
        let mut denom = 0usize;
        for pi in &sorted_callers {
            if pi.path_len > 0 {
                dyn_ratio_sum += (pi.dyn_edges + pi.fnptr_edges) as f64 / pi.path_len as f64;
                gen_args_per_edge_sum += if pi.path_len > 0 {
                    pi.generic_args_len_sum as f64 / pi.path_len as f64
                } else {
                    0.0
                };
                denom += 1;
            }
        }
        let path_dyn_ratio_avg = if denom == 0 { 0.0 } else { dyn_ratio_sum / denom as f64 };
        let path_generic_args_avg = if denom == 0 {
            0.0
        } else {
            gen_args_per_edge_sum / denom as f64
        };

        // RQ3: Generics & instantiation metrics for target functions
        let mut target_callees: HashSet<FunctionInstance<'tcx>> = HashSet::new();
        let mut kind_counts: HashMap<&'static str, usize> = HashMap::new();
        for cs in &self.call_sites {
            if crate::callgraph::utils::matches_function_path(tcx, cs.callee(), target_path, self.without_args) {
                target_callees.insert(cs.callee());
                let k = match cs.call_kind() {
                    crate::callgraph::types::CallKind::Direct => "Direct",
                    crate::callgraph::types::CallKind::FnPtr => "FnPtr",
                    crate::callgraph::types::CallKind::DynTrait => "DynTrait",
                };
                *kind_counts.entry(k).or_default() += 1;
            }
        }
        let variants_count = target_callees.len();
        let unique_def_ids_count = {
            let mut s: HashSet<rustc_hir::def_id::DefId> = HashSet::new();
            for f in &target_callees {
                s.insert(f.def_id());
            }
            s.len()
        };
        // variants per def histogram
        let mut per_def_counts: HashMap<rustc_hir::def_id::DefId, usize> = HashMap::new();
        for f in &target_callees {
            *per_def_counts.entry(f.def_id()).or_default() += 1;
        }
        let mut variants_per_def_hist: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
        for (_def, cnt) in per_def_counts.iter() {
            *variants_per_def_hist.entry(*cnt).or_default() += 1;
        }
        // unique callee crates
        let mut callee_crates: HashSet<rustc_hir::def_id::CrateNum> = HashSet::new();
        for f in &target_callees {
            callee_crates.insert(f.def_id().krate);
        }
        let variant_crates_unique_count = callee_crates.len();
        let mut generic_len_hist: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
        let mut generic_nonzero = 0usize;
        for f in &target_callees {
            let len = f.instance().map(|inst| inst.args.len()).unwrap_or(0);
            *generic_len_hist.entry(len).or_default() += 1;
            if len > 0 {
                generic_nonzero += 1;
            }
        }
        let generic_variant_ratio = if variants_count == 0 {
            0.0
        } else {
            generic_nonzero as f64 / variants_count as f64
        };
        let mut kind_counts_btree: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (k, v) in kind_counts.into_iter() {
            kind_counts_btree.insert(k.to_string(), v);
        }

        // Create the full result object
        let result = json!({
            "target": target_path,
            "total_callers": sorted_callers.len(),
            "callers": caller_entries,
            "rq3_generics": {
                "target_variants_count": variants_count,
                "target_unique_def_ids_count": unique_def_ids_count,
                "variants_per_def_histogram": variants_per_def_hist,
                "variant_crates_unique_count": variant_crates_unique_count,
                "generic_variant_ratio": generic_variant_ratio,
                "generic_args_len_histogram": generic_len_hist,
                "call_kind_counts": kind_counts_btree,
                "path_dyn_ratio_avg": path_dyn_ratio_avg,
                "path_generic_args_avg": path_generic_args_avg
            }
        });

        // Format as pretty-printed JSON
        serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string())
    }
}

pub(crate) fn output_call_graph_result<'tcx>(
    call_graph: &CallGraph<'tcx>,
    tcx: TyCtxt<'tcx>,
    options: &crate::args::CGArgs,
) {
    let crate_name = tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).to_string();

    let output_dir = options
        .output_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("./target"));
    let output_path = output_dir.join(format!("{crate_name}-callgraph.txt"));

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
    } else {
        let formatted_callgraph = call_graph.format_call_graph(tcx);

        match write_to_file(&output_path, |file| write!(file, "{formatted_callgraph}")) {
            Ok(_) => tracing::info!("Call graph written to {}", output_path.display()),
            Err(e) => tracing::error!("Failed to write call graph: {}", e),
        }
    }

    if options.cg_debug {
        let debug_path = output_dir.join(format!("{crate_name}-callgraph-debug.txt"));
        let _ = write_to_file(&debug_path, |file| {
            writeln!(file, "call_graph: {:#?}", call_graph.call_sites)
        });
    }
}

// Helper function to output callers result (reduces code duplication)
pub(crate) fn output_callers_result<'tcx>(
    call_graph: &CallGraph<'tcx>,
    tcx: TyCtxt<'tcx>,
    target: &str,
    callers: Vec<PathInfo<'tcx>>,
    options: &crate::args::CGArgs,
    file_prefix: &str,
) {
    // Get output directory
    let output_dir = options
        .output_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("./target"));

    // Determine output format (text or JSON)
    if options.json_output {
        // Generate JSON output for callers
        let callers_json = call_graph.format_callers_as_json(tcx, target, callers);

        // Output to JSON file
        let json_output_path = output_dir.join(format!("{file_prefix}.json"));

        if let Err(e) = std::fs::write(&json_output_path, callers_json) {
            tracing::error!("Failed to write callers to JSON file: {:?}", e);
        } else {
            tracing::info!("Callers JSON output written to: {:?}", json_output_path);
        }
    } else {
        // Generate text output for callers
        let callers_output = call_graph.format_callers(tcx, target, callers);

        // Output to text file
        let output_path = output_dir.join(format!("{file_prefix}.txt"));

        if let Err(e) = std::fs::write(&output_path, callers_output) {
            tracing::error!("Failed to write callers to file: {:?}", e);
        } else {
            tracing::info!("Callers output written to: {:?}", output_path);
        }
    }
}

/// Write content to a specified file and log the result
///
/// # Parameters
/// * `path` - Path to the output file
/// * `write_fn` - Closure that handles the actual writing
///
/// # Returns
/// * `io::Result<()>` - Ok(()) on success, Err on failure
fn write_to_file<P, F>(path: P, write_fn: F) -> io::Result<()>
where
    P: AsRef<Path>,
    F: FnOnce(&mut std::fs::File) -> io::Result<()>,
{
    // Ensure parent directory exists
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create and write to the file
    let mut file = std::fs::File::create(&path)?;
    write_fn(&mut file)?;

    // Log success message
    tracing::info!("Successfully wrote to file: {}", path.as_ref().display());
    Ok(())
}
