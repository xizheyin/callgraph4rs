use crate::callgraph::utils::get_crate_version;
use crate::callgraph::CallGraph;
use rustc_middle::ty::TyCtxt;
use serde_json::json;
use std::collections::HashMap;

use super::function::FunctionInstance;
use super::types::CallSite;

impl<'tcx> CallGraph<'tcx> {
    /// Convert function instance to readable string
    pub(crate) fn function_instance_to_string(
        &self,
        tcx: TyCtxt<'tcx>,
        instance: FunctionInstance<'tcx>,
    ) -> String {
        match instance {
            FunctionInstance::Instance(inst) => {
                let def_id = inst.def_id();
                // Get readable function name and hash
                let path_hash = format!("{:?}", tcx.def_path_hash(def_id));

                // Determine whether to include generic arguments based on the without_args option
                let path_str = if !self.without_args && inst.args.len() > 0 {
                    // Include generic parameter information
                    tcx.def_path_str_with_args(def_id, inst.args)
                } else {
                    // Skip generic parameter information
                    tcx.def_path_str(def_id)
                };

                // Append the hash to the path string
                format!("{} [{}]", path_str, path_hash)
            }
            FunctionInstance::NonInstance(def_id) => {
                // For non-instances, only show the path and hash
                let path_hash = format!("{}", tcx.def_path_hash(def_id).0);
                format!(
                    "{} (non-instance) [{}]",
                    tcx.def_path_str(def_id),
                    path_hash
                )
            }
        }
    }

    /// Format the call graph as readable text
    pub(crate) fn format_call_graph(&self, tcx: TyCtxt<'tcx>) -> String {
        let mut result = String::new();

        result.push_str("Call Graph:\n");
        result.push_str("===========\n\n");

        // Organize calls by caller
        let mut calls_by_caller: HashMap<FunctionInstance<'tcx>, Vec<&CallSite<'tcx>>> =
            HashMap::new();

        for call_site in &self.call_sites {
            calls_by_caller
                .entry(call_site.caller())
                .or_default()
                .push(call_site);
        }

        // Sort callers to get consistent output
        let mut callers: Vec<FunctionInstance<'tcx>> = calls_by_caller.keys().cloned().collect();
        callers.sort_by_key(|caller| format!("{:?}", caller));

        for caller in callers {
            // Get caller name
            let caller_name = self.function_instance_to_string(tcx, caller);
            result.push_str(&format!("Function: {}\n", caller_name));

            // Get all calls from this caller
            if let Some(calls) = calls_by_caller.get(&caller) {
                // Sort by callee and constraint count
                let mut sorted_calls = calls.clone();
                sorted_calls.sort_by(|a, b| {
                    let a_name = self.function_instance_to_string(tcx, a.callee());
                    let b_name = self.function_instance_to_string(tcx, b.callee());
                    a_name
                        .cmp(&b_name)
                        .then_with(|| a.constraint_count().cmp(&b.constraint_count()))
                });

                // Output call information
                for call in sorted_calls {
                    let callee_name = self.function_instance_to_string(tcx, call.callee());
                    result.push_str(&format!(
                        "  -> {} [constraint: {}]\n",
                        callee_name,
                        call.constraint_count()
                    ));
                }

                result.push_str("\n");
            }
        }

        result
    }

    /// Format caller information as readable text
    pub(crate) fn format_callers(
        &self,
        tcx: TyCtxt<'tcx>,
        target_path: &str,
        callers: Vec<FunctionInstance<'tcx>>,
    ) -> String {
        let mut result = String::new();

        result.push_str(&format!(
            "Callers of functions matching '{}':\n",
            target_path
        ));
        result.push_str("==================================\n\n");

        // Sort callers to get consistent output
        let mut sorted_callers: Vec<FunctionInstance<'tcx>> = callers;
        sorted_callers.sort_by_key(|caller| format!("{:?}", caller));

        for caller in &sorted_callers {
            let caller_name = self.function_instance_to_string(tcx, *caller);
            result.push_str(&format!("- {}\n", caller_name));
        }

        result.push_str(&format!(
            "\nTotal: {} callers found\n",
            sorted_callers.len()
        ));
        result
    }

    /// Format the call graph as JSON
    pub(crate) fn format_call_graph_as_json(&self, tcx: TyCtxt<'tcx>) -> String {
        // Create a map to organize calls by caller
        let mut calls_by_caller: HashMap<FunctionInstance<'tcx>, Vec<&CallSite<'tcx>>> =
            HashMap::new();

        for call_site in &self.call_sites {
            calls_by_caller
                .entry(call_site.caller())
                .or_default()
                .push(call_site);
        }

        // Sort callers to get consistent output
        let mut callers: Vec<FunctionInstance<'tcx>> = calls_by_caller.keys().cloned().collect();
        callers.sort_by_key(|caller| format!("{:?}", caller));

        // Create the JSON array to hold all entries
        let mut json_entries = Vec::new();

        for caller in callers {
            // Get caller name and information
            let caller_name = self.function_instance_to_string(tcx, caller);
            let caller_def_id = caller.def_id();
            let caller_path = tcx.def_path_str(caller_def_id);

            // Get all calls from this caller
            if let Some(calls) = calls_by_caller.get(&caller) {
                // Sort by callee for consistent output
                let mut sorted_calls = calls.clone();
                sorted_calls.sort_by(|a, b| {
                    let a_name = self.function_instance_to_string(tcx, a.callee());
                    let b_name = self.function_instance_to_string(tcx, b.callee());
                    a_name
                        .cmp(&b_name)
                        .then_with(|| a.constraint_count().cmp(&b.constraint_count()))
                });

                // Create an array of callee objects
                let mut callees = Vec::new();
                for call in sorted_calls {
                    let callee_name = self.function_instance_to_string(tcx, call.callee());
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
                        "path_hash": format!("{}", tcx.def_path_hash(callee_def_id).0)
                    }));
                }

                // Get actual version information for caller
                let caller_version = get_crate_version(tcx, caller_def_id);

                // Calculate the maximum constraint depth
                let max_constraint_depth = calls
                    .iter()
                    .map(|c| c.constraint_count())
                    .max()
                    .unwrap_or(0);

                // Create the full entry with caller and callees
                let entry = json!({
                    "caller": {
                        "name": caller_name,
                        "version": caller_version,
                        "path": caller_path,
                        "constraint_depth": max_constraint_depth,
                        "path_hash": format!("{}", tcx.def_path_hash(caller_def_id).0)
                    },
                    "callee": callees
                });

                json_entries.push(entry);
            }
        }

        // Format the entire array as a pretty-printed JSON string
        serde_json::to_string_pretty(&json_entries).unwrap_or_else(|_| "[]".to_string())
    }

    /// Format caller information as JSON
    pub(crate) fn format_callers_as_json(
        &self,
        tcx: TyCtxt<'tcx>,
        target_path: &str,
        callers: Vec<FunctionInstance<'tcx>>,
    ) -> String {
        // Sort callers to get consistent output
        let mut sorted_callers: Vec<FunctionInstance<'tcx>> = callers;
        sorted_callers.sort_by_key(|caller| format!("{:?}", caller));

        // Create array for caller information
        let mut caller_entries = Vec::new();

        for caller in &sorted_callers {
            let caller_name = self.function_instance_to_string(tcx, *caller);
            let caller_def_id = caller.def_id();
            let caller_path = tcx.def_path_str(caller_def_id);

            // Get version information
            let version = get_crate_version(tcx, caller_def_id);

            // Add caller entry
            caller_entries.push(json!({
                "name": caller_name,
                "version": version,
                "path": caller_path,
                "path_hash": format!("{}", tcx.def_path_hash(caller_def_id).0)
            }));
        }

        // Create the full result object
        let result = json!({
            "target": target_path,
            "total_callers": sorted_callers.len(),
            "callers": caller_entries
        });

        // Format as pretty-printed JSON
        serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string())
    }
}
