use rustc_hir::{def, def_id::DefId};
use rustc_middle::ty::{Instance, TyCtxt, TypeFoldable, TypingEnv};
use rustc_middle::{
    mir::{self, Terminator, TerminatorKind},
    ty::{self},
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::constraint_utils::{self, BlockPath};

pub(crate) struct CallGraph<'tcx> {
    _all_generic_instances: Vec<FunctionInstance<'tcx>>,
    instances: VecDeque<FunctionInstance<'tcx>>,
    pub call_sites: Vec<CallSite<'tcx>>,
}

impl<'tcx> CallGraph<'tcx> {
    fn new(all_generic_instances: Vec<FunctionInstance<'tcx>>) -> Self {
        Self {
            _all_generic_instances: all_generic_instances.clone(),
            instances: all_generic_instances.into_iter().collect(),
            call_sites: Vec::new(),
        }
    }

    /// Deduplicate call sites, keeping only the one with the minimum constraint count
    /// for each unique caller-callee pair
    pub fn deduplicate_call_sites(&mut self) {
        // Create a map to track the call site with minimum constraint_cnt for each caller-callee pair
        let mut min_constraints: HashMap<(FunctionInstance<'tcx>, FunctionInstance<'tcx>), usize> =
            HashMap::new();
        let mut min_indices: HashMap<(FunctionInstance<'tcx>, FunctionInstance<'tcx>), usize> =
            HashMap::new();

        // Find minimum constraint count for each caller-callee pair
        for (index, call_site) in self.call_sites.iter().enumerate() {
            let key = (call_site._caller, call_site.callee);

            if let Some(existing_cnt) = min_constraints.get(&key) {
                if call_site.constraint_cnt < *existing_cnt {
                    min_constraints.insert(key, call_site.constraint_cnt);
                    min_indices.insert(key, index);
                }
            } else {
                min_constraints.insert(key, call_site.constraint_cnt);
                min_indices.insert(key, index);
            }
        }

        // Keep only the call sites with minimum constraint counts
        let indices_to_keep: HashSet<usize> = min_indices.values().cloned().collect();

        // Create a new call_sites vector with only the deduplicated entries
        let mut deduplicated_call_sites = Vec::new();

        for (index, call_site) in self.call_sites.iter().enumerate() {
            if indices_to_keep.contains(&index) {
                deduplicated_call_sites.push(call_site.clone());
            }
        }

        tracing::info!(
            "Deduplicated call sites: {} -> {} entries",
            self.call_sites.len(),
            deduplicated_call_sites.len()
        );

        self.call_sites = deduplicated_call_sites.into_iter().collect();
    }

    /// Format the call graph as readable text
    pub fn format_call_graph(&self, tcx: TyCtxt<'tcx>) -> String {
        let mut result = String::new();

        result.push_str("Call Graph:\n");
        result.push_str("===========\n\n");

        // Organize calls by caller
        let mut calls_by_caller: HashMap<FunctionInstance<'tcx>, Vec<&CallSite<'tcx>>> =
            HashMap::new();

        for call_site in &self.call_sites {
            calls_by_caller
                .entry(call_site._caller)
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
                    let a_name = self.function_instance_to_string(tcx, a.callee);
                    let b_name = self.function_instance_to_string(tcx, b.callee);
                    a_name
                        .cmp(&b_name)
                        .then_with(|| a.constraint_cnt.cmp(&b.constraint_cnt))
                });

                // Output call information
                for call in sorted_calls {
                    let callee_name = self.function_instance_to_string(tcx, call.callee);
                    result.push_str(&format!(
                        "  -> {} [constraint: {}]\n",
                        callee_name, call.constraint_cnt
                    ));
                }

                result.push_str("\n");
            }
        }

        result
    }

    /// Convert function instance to readable string
    fn function_instance_to_string(
        &self,
        tcx: TyCtxt<'tcx>,
        instance: FunctionInstance<'tcx>,
    ) -> String {
        match instance {
            FunctionInstance::Instance(inst) => {
                let def_id = inst.def_id();
                // Get readable function name

                // Add generic parameter information, if any
                if inst.args.len() > 0 {
                    tcx.def_path_str_with_args(def_id, inst.args)
                } else {
                    tcx.def_path_str(def_id)
                }
            }
            FunctionInstance::NonInstance(def_id) => {
                // For non-instances, only show the path
                format!("{} (non-instance)", tcx.def_path_str(def_id))
            }
        }
    }

    /// Find all functions that directly or indirectly call the specified function
    pub fn find_callers(
        &self,
        tcx: TyCtxt<'tcx>,
        target_path: &str,
    ) -> Option<Vec<FunctionInstance<'tcx>>> {
        // First find functions that match the specified path
        let target_functions: Vec<FunctionInstance<'tcx>> = self
            .call_sites
            .iter()
            .map(|call_site| call_site.callee)
            .filter(|func| {
                // Get complete function path (including generic parameters)
                let full_func_path = self.function_instance_to_string(tcx, *func);

                // Also get the basic path without generic parameters
                let base_path = match func {
                    FunctionInstance::Instance(inst) => tcx.def_path_str(inst.def_id()),
                    FunctionInstance::NonInstance(def_id) => tcx.def_path_str(*def_id),
                };

                // If the target path contains '<', assume the user specified a complete path with generic parameters
                if target_path.contains("<") {
                    // If there are angle brackets, match complete path or basic path
                    tracing::trace!("base_path: {}", base_path);
                    tracing::trace!("full_func_path: {}", full_func_path);
                    base_path.contains(target_path) || full_func_path.contains(target_path)
                } else {
                    // If there are no angle brackets, remove all generic parameter parts from function paths
                    // Remove all ::<...> parts from base_path and full_func_path

                    // Process generic parameters in path
                    let process_path = |path: &str| -> String {
                        let mut result = String::new();
                        let mut in_generic = false;
                        let mut angle_bracket_count = 0;
                        let mut skip_from_index = 0;

                        // Traverse the string, identify and remove generic parameter parts
                        for (i, c) in path.char_indices() {
                            if c == '<' {
                                if !in_generic && i >= 2 && &path[i - 2..i] == "::" {
                                    // Find the starting position of generic parameters
                                    in_generic = true;
                                    angle_bracket_count = 1;
                                    skip_from_index = i - 2; // Including ::
                                    result.truncate(skip_from_index);
                                } else if in_generic {
                                    angle_bracket_count += 1;
                                }
                            } else if c == '>' && in_generic {
                                angle_bracket_count -= 1;
                                if angle_bracket_count == 0 {
                                    // End of generic parameters
                                    in_generic = false;
                                }
                            } else if !in_generic && skip_from_index <= i {
                                // Not within generic parameters, add to result
                                result.push(c);
                            }
                        }

                        result
                    };

                    // Clean both paths
                    let clean_base_path = process_path(&base_path);
                    let clean_full_path = process_path(&full_func_path);

                    tracing::trace!("clean_base_path: {}", clean_base_path);
                    tracing::trace!("clean_full_path: {}", clean_full_path);
                    // Use cleaned paths for matching
                    clean_base_path.contains(target_path) || clean_full_path.contains(target_path)
                }
            })
            .collect();

        if target_functions.is_empty() {
            tracing::warn!("No function found matching path: {}", target_path);
            return None;
        }

        tracing::info!(
            "Found {} functions matching path: {}",
            target_functions.len(),
            target_path
        );
        for func in &target_functions {
            tracing::info!(
                "Matched function: {}",
                self.function_instance_to_string(tcx, *func)
            );
        }

        // Create mapping from callee to callers
        let mut callee_to_callers: HashMap<
            FunctionInstance<'tcx>,
            HashSet<FunctionInstance<'tcx>>,
        > = HashMap::new();
        for call_site in &self.call_sites {
            callee_to_callers
                .entry(call_site.callee)
                .or_default()
                .insert(call_site._caller);
        }

        // Find all direct and indirect callers
        let mut all_callers: HashSet<FunctionInstance<'tcx>> = HashSet::new();
        let mut queue: VecDeque<FunctionInstance<'tcx>> = target_functions.into_iter().collect();
        let mut processed: HashSet<FunctionInstance<'tcx>> = HashSet::new();

        while let Some(current) = queue.pop_front() {
            if processed.contains(&current) {
                continue;
            }
            processed.insert(current);

            if let Some(callers) = callee_to_callers.get(&current) {
                for caller in callers {
                    if !processed.contains(caller) {
                        all_callers.insert(*caller);
                        queue.push_back(*caller);
                    }
                }
            }
        }

        if all_callers.is_empty() {
            tracing::warn!("No callers found for the specified function");
            return None;
        }

        Some(all_callers.into_iter().collect())
    }

    /// Format caller information as readable text
    pub fn format_callers(
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
    pub fn format_call_graph_as_json(&self, tcx: TyCtxt<'tcx>) -> String {
        // Create a map to organize calls by caller
        let mut calls_by_caller: HashMap<FunctionInstance<'tcx>, Vec<&CallSite<'tcx>>> =
            HashMap::new();

        for call_site in &self.call_sites {
            calls_by_caller
                .entry(call_site._caller)
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
                    let a_name = self.function_instance_to_string(tcx, a.callee);
                    let b_name = self.function_instance_to_string(tcx, b.callee);
                    a_name
                        .cmp(&b_name)
                        .then_with(|| a.constraint_cnt.cmp(&b.constraint_cnt))
                });

                // Create an array of callee objects
                let mut callees = Vec::new();
                for call in sorted_calls {
                    let callee_name = self.function_instance_to_string(tcx, call.callee);
                    let callee_def_id = call.callee.def_id();
                    let callee_path = tcx.def_path_str(callee_def_id);

                    // Get actual version information for this callee
                    let version = get_crate_version(tcx, callee_def_id);

                    // Add callee entry
                    callees.push(json!({
                        "name": callee_name,
                        "version": version,
                        "path": callee_path,
                        "constraint_depth": call.constraint_cnt
                    }));
                }

                // Get actual version information for caller
                let caller_version = get_crate_version(tcx, caller_def_id);

                // Calculate the maximum constraint depth
                let max_constraint_depth =
                    calls.iter().map(|c| c.constraint_cnt).max().unwrap_or(0);

                // Create the full entry with caller and callees
                let entry = json!({
                    "caller": {
                        "name": caller_name,
                        "version": caller_version,
                        "path": caller_path,
                        "constraint_depth": max_constraint_depth
                    },
                    "callee": callees
                });

                json_entries.push(entry);
            }
        }

        // Format the entire array as a pretty-printed JSON string
        serde_json::to_string_pretty(&json_entries).unwrap_or_else(|_| "[]".to_string())
    }
}

// Get version information for a specific DefId from TyCtxt
fn get_crate_version<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> String {
    // Get the crate number for this DefId
    let crate_num = def_id.krate;

    // Try to get the crate name
    let crate_name = tcx.crate_name(crate_num);

    // Check if we can get version from crate disambiguator (hash)
    let crate_hash = tcx.crate_hash(crate_num);

    // For built-in crates and std library, we can use the Rust version
    if crate_num == rustc_hir::def_id::LOCAL_CRATE {
        // This is the current crate being analyzed
        // Try to get version from environment if available
        match option_env!("CARGO_PKG_VERSION") {
            Some(version) => return version.to_string(),
            None => {}
        }
    }

    // Look for version patterns in the crate name (some crates include version in name)
    // Format: name-x.y.z
    let crate_name_str = crate_name.to_string();
    if let Some(idx) = crate_name_str.rfind('-') {
        let potential_version = &crate_name_str[idx + 1..];
        if potential_version
            .chars()
            .next()
            .map_or(false, |c| c.is_digit(10))
        {
            return potential_version.to_string();
        }
    }

    // If we can't find a proper version, use the crate hash as a unique identifier
    format!("0.0.0-{}", crate_hash.to_string().split_at(8).0)
}

#[derive(Debug, Clone)]
pub struct CallSite<'tcx> {
    _caller: FunctionInstance<'tcx>,
    callee: FunctionInstance<'tcx>,
    constraint_cnt: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionInstance<'tcx> {
    Instance(ty::Instance<'tcx>),
    NonInstance(DefId),
}

impl<'tcx> FunctionInstance<'tcx> {
    fn new_instance(instance: ty::Instance<'tcx>) -> Self {
        Self::Instance(instance)
    }

    fn new_non_instance(def_id: DefId) -> Self {
        Self::NonInstance(def_id)
    }

    fn instance(&self) -> Option<ty::Instance<'tcx>> {
        match self {
            Self::Instance(instance) => Some(*instance),
            Self::NonInstance(_) => None,
        }
    }

    fn _non_instance(&self) -> Option<DefId> {
        match self {
            Self::Instance(_) => None,
            Self::NonInstance(def_id) => Some(*def_id),
        }
    }

    fn def_id(&self) -> DefId {
        match self {
            Self::Instance(instance) => instance.def_id(),
            Self::NonInstance(def_id) => *def_id,
        }
    }

    fn is_instance(&self) -> bool {
        match self {
            Self::Instance(_) => true,
            Self::NonInstance(_) => false,
        }
    }
    fn is_non_instance(&self) -> bool {
        !self.is_instance()
    }

    fn collect_callsites(&self, tcx: ty::TyCtxt<'tcx>) -> Vec<CallSite<'tcx>> {
        let def_id = self.def_id();

        if self.is_non_instance() {
            tracing::warn!("skip non-instance function: {:?}", self);
            return Vec::new();
        }

        if !tcx.is_mir_available(def_id) {
            tracing::warn!("skip nobody function: {:?}", def_id);
            return Vec::new();
        }
        let constraints = get_constraints(tcx, def_id);
        self.extract_function_call(tcx, &def_id, constraints)
    }

    /// Extract information about all function calls in `function`
    fn extract_function_call(
        &self,
        tcx: ty::TyCtxt<'tcx>,
        caller_id: &DefId,
        constraints: HashMap<mir::BasicBlock, BlockPath>,
    ) -> Vec<CallSite<'tcx>> {
        use mir::visit::Visitor;

        #[derive(Clone)]
        struct SearchFunctionCall<'tcx, 'local> {
            tcx: ty::TyCtxt<'tcx>,
            caller_instance: &'local FunctionInstance<'tcx>,
            caller_body: &'local mir::Body<'tcx>,
            callees: Vec<CallSite<'tcx>>,
            constraints: HashMap<mir::BasicBlock, BlockPath>,
            current_bb: mir::BasicBlock,
        }

        impl<'tcx, 'local> SearchFunctionCall<'tcx, 'local> {
            fn new(
                tcx: ty::TyCtxt<'tcx>,
                caller_instance: &'local FunctionInstance<'tcx>,
                caller_body: &'local mir::Body<'tcx>,
                constraints: HashMap<mir::BasicBlock, BlockPath>,
            ) -> Self {
                SearchFunctionCall {
                    tcx,
                    caller_instance,
                    caller_body,
                    callees: Vec::default(),
                    constraints,
                    current_bb: mir::BasicBlock::from_usize(0),
                }
            }
        }

        impl<'tcx, 'local> Visitor<'tcx> for SearchFunctionCall<'tcx, 'local> {
            fn visit_basic_block_data(
                &mut self,
                block: mir::BasicBlock,
                data: &mir::BasicBlockData<'tcx>,
            ) {
                self.current_bb = block;
                self.super_basic_block_data(block, data);
            }

            fn visit_terminator(
                &mut self,
                terminator: &Terminator<'tcx>,
                _location: mir::Location,
            ) {
                if let TerminatorKind::Call { func, .. } = &terminator.kind {
                    tracing::debug!(
                        "Found Call => callee: {:?}, func.ty: {:?}",
                        func,
                        func.ty(self.caller_body, self.tcx)
                    );

                    use mir::Operand::*;
                    let typing_env =
                        TypingEnv::post_analysis(self.tcx, self.caller_instance.def_id());

                    let before_mono_ty = func.ty(self.caller_body, self.tcx);
                    let monod_result = monomorphize(
                        self.tcx,
                        typing_env,
                        self.caller_instance.instance().expect("instance is None"),
                        before_mono_ty,
                    );

                    let callee = if let Err(err) = monod_result {
                        tracing::warn!("Monomorphization failed: {:?}", err);
                        match func {
                            Constant(_) => match before_mono_ty.kind() {
                                ty::TyKind::FnDef(def_id, _) => {
                                    tracing::warn!(
                                        "Callee {:?} is not monomorphized, using non-instance",
                                        def_id
                                    );
                                    Some(FunctionInstance::new_non_instance(*def_id))
                                }
                                _ => None,
                            },
                            Move(_) | Copy(_) => {
                                tracing::warn!("skip move or copy: {:?}", func);
                                None
                            }
                        }
                    } else {
                        let monod_ty = monod_result.unwrap();

                        match func {
                            Constant(_) => match monod_ty.kind() {
                                ty::TyKind::FnDef(def_id, monoed_args) => {
                                    match self.tcx.def_kind(def_id) {
                                        def::DefKind::Fn | def::DefKind::AssocFn => {
                                            tracing::debug!("Try resolve instance: {:?}", monod_ty);
                                            let instance_result = ty::Instance::try_resolve(
                                                self.tcx,
                                                typing_env,
                                                *def_id,
                                                monoed_args,
                                            );

                                            match instance_result {
                                                Err(err) => {
                                                    tracing::error!(
                                                        "Instance [{:?}] resolution error: {:?}",
                                                        monod_ty,
                                                        err
                                                    );
                                                    None
                                                }
                                                Ok(opt_instance) => {
                                                    if let Some(instance) = opt_instance {
                                                        tracing::info!(
                                                            "Resolved instance successfully: {:?}",
                                                            instance
                                                        );
                                                        Some(FunctionInstance::new_instance(
                                                            instance,
                                                        ))
                                                    } else {
                                                        tracing::warn!(
                                                            "Resolve [{:#?}] failed, try trivial resolve",
                                                            monod_ty
                                                        );
                                                        trivial_resolve(self.tcx, *def_id).or_else(|| {
                                                            tracing::warn!("Trivial resolve [{:?}] also failed, using non-instance", def_id);
                                                            Some(FunctionInstance::new_non_instance(*def_id))
                                                        })
                                                    }
                                                }
                                            }
                                        }
                                        other => {
                                            tracing::error!(
                                                "internal error: unknown call type: {:?}",
                                                other
                                            );
                                            None
                                        }
                                    }
                                }
                                _ => {
                                    tracing::error!(
                                        "internal error: unexpected function type: {:?}",
                                        monod_ty
                                    );
                                    None
                                }
                            },
                            // Move or copy operands
                            Move(_) | Copy(_) => {
                                tracing::warn!("skip move or copy: {:?}", func);
                                None
                            }
                        }
                    };

                    // If callee function is found, add to the call list
                    if let Some(callee) = callee {
                        self.callees.push(CallSite {
                            _caller: *self.caller_instance,
                            callee,
                            constraint_cnt: self.constraints[&self.current_bb].constraints,
                        });
                    }
                }
            }
        }

        let caller_body = tcx.optimized_mir(caller_id);
        let mut search_callees = SearchFunctionCall::new(tcx, self, caller_body, constraints);
        search_callees.visit_body(caller_body);
        search_callees.callees
    }
}

pub fn collect_generic_instances(tcx: ty::TyCtxt<'_>) -> Vec<FunctionInstance<'_>> {
    let mut instances = Vec::new();
    for def_id in tcx.hir_body_owners() {
        let ty = tcx.type_of(def_id).skip_binder();
        if let ty::TyKind::FnDef(def_id, args) = ty.kind() {
            let instance = ty::Instance::try_resolve(
                tcx,
                TypingEnv::post_analysis(tcx, *def_id),
                *def_id,
                args,
            );
            if let Ok(Some(instance)) = instance {
                instances.push(FunctionInstance::new_instance(instance));
            }
        }
    }
    instances
}

fn trivial_resolve(tcx: ty::TyCtxt<'_>, def_id: DefId) -> Option<FunctionInstance<'_>> {
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

pub fn perform_mono_analysis<'tcx>(
    tcx: ty::TyCtxt<'tcx>,
    instances: Vec<FunctionInstance<'tcx>>,
    options: &crate::args::CGArgs,
) -> CallGraph<'tcx> {
    let mut call_graph = CallGraph::new(instances);
    let mut visited = HashSet::new();

    while let Some(instance) = call_graph.instances.pop_front() {
        if visited.contains(&instance) {
            continue;
        }
        visited.insert(instance);

        let call_sites = instance.collect_callsites(tcx);
        for call_site in call_sites {
            call_graph.instances.push_back(call_site.callee);
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
            let callers_output = call_graph.format_callers(tcx, target_path, callers);

            // Output to file
            let output_path = options
                .output_dir
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from("./target"))
                .join("callers.txt");

            if let Err(e) = std::fs::write(&output_path, callers_output) {
                tracing::error!("Failed to write callers to file: {:?}", e);
            } else {
                tracing::info!("Callers output written to: {:?}", output_path);
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

fn get_constraints(tcx: ty::TyCtxt, def_id: DefId) -> HashMap<mir::BasicBlock, BlockPath> {
    let mir = tcx.optimized_mir(def_id);
    constraint_utils::compute_shortest_paths(mir)
}
