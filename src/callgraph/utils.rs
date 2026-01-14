use rustc_hir::def_id::{CrateNum, DefId};
use rustc_middle::ty::TyCtxt;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::callgraph::{function::FunctionInstance, types::PathInfo};

use super::types::CallGraph;

// Get version information for a specific DefId from TyCtxt
pub(crate) fn get_crate_version<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> String {
    // Try to get the crate name
    let crate_num = def_id.krate;
    let crate_name = tcx.crate_name(crate_num);

    // Check if we can get version from crate disambiguator (hash)
    let crate_hash = tcx.crate_hash(crate_num);

    // For built-in crates and std library, we can use the Rust version
    if crate_num == rustc_hir::def_id::LOCAL_CRATE {
        // This is the current crate being analyzed
        // Try to get version from environment if available
        if let Some(version) = option_env!("CARGO_PKG_VERSION") {
            return version.to_string();
        }
    }

    // Look for version patterns in the crate name (some crates include version in name)
    // Crates in crates.io have a version in name
    // Format: name-x.y.z
    let crate_name_str = crate_name.to_string();
    if let Some(idx) = crate_name_str.rfind('-') {
        let potential_version = &crate_name_str[idx + 1..];
        if potential_version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return potential_version.to_string();
        }
    }

    // If we can't find a proper version, use the crate hash as a unique identifier
    format!("0.0.0-{}", crate_hash.to_string().split_at(8).0)
}

/// Strip generic parameters (::<...>) from a function path
pub fn strip_generics_from_path(path: &str) -> String {
    let mut result = String::new();
    let mut in_generic = false;
    let mut angle_bracket_count = 0;

    let chars: Vec<(usize, char)> = path.char_indices().collect();

    for (i, c) in chars {
        if c == '<' {
            // Check for "::<" pattern
            let is_start_generic = !in_generic && i >= 2 && &path[i - 2..i] == "::";

            if is_start_generic {
                in_generic = true;
                angle_bracket_count = 1;
                // Remove the "::" that immediately precedes "<"
                if result.ends_with("::") {
                    let new_len = result.len() - 2;
                    result.truncate(new_len);
                }
            } else if in_generic {
                angle_bracket_count += 1;
            } else {
                result.push(c);
            }
        } else if c == '>' && in_generic {
            angle_bracket_count -= 1;
            if angle_bracket_count == 0 {
                in_generic = false;
            }
        } else if !in_generic {
            result.push(c);
        }
    }
    result
}

fn strip_args_from_path(path: &str) -> &str {
    match path.find('(') {
        Some(idx) => &path[..idx],
        None => path,
    }
}

fn normalize_path_for_match(path: &str, without_args: bool) -> String {
    let trimmed = path.trim();
    let no_args = if without_args {
        strip_args_from_path(trimmed)
    } else {
        trimmed
    };
    strip_generics_from_path(no_args)
}

fn segment_match(candidate: &str, target: &str) -> bool {
    let cand_segs: Vec<&str> = candidate.split("::").filter(|s| !s.is_empty()).collect();
    let targ_segs: Vec<&str> = target.split("::").filter(|s| !s.is_empty()).collect();

    if targ_segs.is_empty() || cand_segs.len() < targ_segs.len() {
        return false;
    }

    if targ_segs.len() == 1 {
        return cand_segs
            .last()
            .is_some_and(|last| *last == targ_segs[0]);
    }

    for i in 0..=(cand_segs.len() - targ_segs.len()) {
        if cand_segs[i..i + targ_segs.len()] == targ_segs[..] {
            return true;
        }
    }

    false
}

/// Check if a function matches the target path description
pub fn matches_function_path<'tcx>(
    tcx: TyCtxt<'tcx>,
    func: FunctionInstance<'tcx>,
    target_path: &str,
    without_args: bool,
) -> bool {
    let full_func_path = func.full_path(tcx, without_args);

    let base_path = match func {
        FunctionInstance::Instance(inst) => tcx.def_path_str(inst.def_id()),
        FunctionInstance::NonInstance(def_id) => tcx.def_path_str(def_id),
    };

    if target_path.contains('<') && (base_path == target_path || full_func_path == target_path) {
        return true;
    }

    let clean_target = normalize_path_for_match(target_path, without_args);
    if clean_target.is_empty() {
        return false;
    }

    let clean_base_path = normalize_path_for_match(&base_path, true);
    let clean_full_path = normalize_path_for_match(&full_func_path, without_args);

    tracing::trace!("clean_target: {}", clean_target);
    tracing::trace!("clean_base_path: {}", clean_base_path);
    tracing::trace!("clean_full_path: {}", clean_full_path);

    segment_match(&clean_base_path, &clean_target) || segment_match(&clean_full_path, &clean_target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_generics_from_path() {
        assert_eq!(
            strip_generics_from_path("std::vec::Vec::<T>::new"),
            "std::vec::Vec::new"
        );
        assert_eq!(
            strip_generics_from_path("my_crate::foo::<i32, f64>::bar"),
            "my_crate::foo::bar"
        );
        assert_eq!(
            strip_generics_from_path("std::option::Option::<std::string::String>::None"),
            "std::option::Option::None"
        );
        // Nested generics
        assert_eq!(strip_generics_from_path("my::func::<Vec::<i32>>"), "my::func");
        // No generics
        assert_eq!(strip_generics_from_path("simple::function"), "simple::function");
        // Edge case: :: not before <
        assert_eq!(strip_generics_from_path("val < 5"), "val < 5");
        // Edge case: starts with generic? (Unlikely in Rust path but good to test)
        assert_eq!(strip_generics_from_path("::<T>"), "");
    }
}

impl<'tcx> CallGraph<'tcx> {
    /// Deduplicate call sites, keeping only the one with the minimum constraint count
    /// for each unique caller-callee pair
    pub fn deduplicate_call_sites(&mut self) {
        // Create a map to track the call site with minimum constraint_cnt for each caller-callee pair
        let mut min_constraints: HashMap<(FunctionInstance<'tcx>, FunctionInstance<'tcx>), usize> = HashMap::new();
        let mut min_indices: HashMap<(FunctionInstance<'tcx>, FunctionInstance<'tcx>), usize> = HashMap::new();

        // Find minimum constraint count for each caller-callee pair
        for (index, call_site) in self.call_sites.iter().enumerate() {
            let key = (call_site.caller(), call_site.callee());

            if let Some(existing_cnt) = min_constraints.get(&key) {
                if call_site.constraint_count() < *existing_cnt {
                    min_constraints.insert(key, call_site.constraint_count());
                    min_indices.insert(key, index);
                }
            } else {
                min_constraints.insert(key, call_site.constraint_count());
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

        tracing::debug!(
            "Deduplicated call sites: {} -> {} entries",
            self.call_sites.len(),
            deduplicated_call_sites.len()
        );

        self.call_sites = deduplicated_call_sites.into_iter().collect();
    }

    /// Find functions that match a predicate and then find all their callers
    fn find_callers_by_predicate<F>(
        &self,
        tcx: TyCtxt<'tcx>,
        target_description: &str,
        predicate: F,
    ) -> Vec<PathInfo<'tcx>>
    where
        F: Fn(FunctionInstance<'tcx>, TyCtxt<'tcx>) -> bool,
    {
        // First find functions that match the predicate
        let target_functions: Vec<FunctionInstance<'tcx>> = self
            .call_sites
            .iter()
            .map(|call_site| call_site.callee())
            .filter(|&func| predicate(func, tcx))
            .collect();

        if target_functions.is_empty() {
            tracing::warn!("No function found matching {}", target_description);
            return Vec::new();
        }

        tracing::debug!("Found {} functions matching", target_functions.len());

        // Create mapping from callee to callers with edge attributes
        // (constraints, package_num, call_kind, generic_args_len)
        let mut callee_to_callers: HashMap<
            FunctionInstance<'tcx>,
            HashMap<FunctionInstance<'tcx>, (usize, usize, crate::callgraph::types::CallKind, usize)>,
        > = HashMap::new();

        for call_site in &self.call_sites {
            let caller = call_site.caller();
            let callee = call_site.callee();
            let constraints = call_site.constraint_count();
            let package_num = call_site.package_num();

            let call_kind = call_site.call_kind();
            let generic_len = callee.instance().map(|inst| inst.args.len()).unwrap_or(0);
            callee_to_callers
                .entry(callee)
                .or_default()
                .entry(caller)
                .and_modify(|(c, p, k, g)| {
                    if constraints < *c {
                        *c = constraints;
                        *p = package_num;
                        *k = call_kind;
                        *g = generic_len;
                    }
                })
                .or_insert((constraints, package_num, call_kind, generic_len));
        }

        // 使用 Dijkstra 算法查找所有直接或间接调用者的最短约束路径
        #[derive(Clone)]
        struct State<'tcx> {
            cost: usize,
            node: FunctionInstance<'tcx>,
            package_sum: usize,
            package_unique: HashSet<CrateNum>,
            depth: usize,
            dyn_edges: usize,
            fnptr_edges: usize,
            generic_args_len_sum: usize,
        }

        impl<'tcx> Eq for State<'tcx> {}

        impl<'tcx> PartialEq for State<'tcx> {
            fn eq(&self, other: &Self) -> bool {
                self.cost == other.cost
            }
        }

        impl<'tcx> Ord for State<'tcx> {
            fn cmp(&self, other: &Self) -> Ordering {
                // BinaryHeap 是最大堆，这里反转比较以实现最小堆行为
                other.cost.cmp(&self.cost)
            }
        }

        impl<'tcx> PartialOrd for State<'tcx> {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        let mut dist: HashMap<FunctionInstance<'tcx>, (usize, usize, usize, usize, usize, usize, usize)> =
            HashMap::new();
        let mut heap: BinaryHeap<State<'tcx>> = BinaryHeap::new();

        for target in &target_functions {
            dist.insert(*target, (0, 0, 0, 0, 0, 0, 0));
            heap.push(State {
                cost: 0,
                node: *target,
                package_sum: 0,
                package_unique: HashSet::new(),
                depth: 0,
                dyn_edges: 0,
                fnptr_edges: 0,
                generic_args_len_sum: 0,
            });
        }

        while let Some(State {
            cost: cur_cost,
            node: cur_node,
            package_sum: cur_pkg,
            package_unique: cur_pkg_unique,
            depth: cur_depth,
            dyn_edges: cur_dyn,
            fnptr_edges: cur_fnptr,
            generic_args_len_sum: cur_genlen,
        }) = heap.pop()
        {
            // skip if the current cost is worse than the best known
            // FIXME: non-negative weights, we can use visited set to skip
            // but current version is more general
            if let Some((best, _, _, _, _, _, _)) = dist.get(&cur_node) {
                if cur_cost > *best {
                    continue;
                }
            }

            // Find all caller
            if let Some(callers) = callee_to_callers.get(&cur_node) {
                for (caller, (edge_cost, edge_pkg, edge_kind, edge_genlen)) in callers {
                    let next_cost = cur_cost + edge_cost;

                    match dist.get(caller) {
                        Some((best, _, _, _, _, _, _)) if next_cost >= *best => {}
                        _ => {
                            let next_pkg = cur_pkg + edge_pkg;
                            let next_depth = cur_depth + 1;
                            let mut next_package_unique = cur_pkg_unique.clone();
                            next_package_unique.insert(caller.def_id().krate);
                            let next_dyn = cur_dyn
                                + if matches!(edge_kind, crate::callgraph::types::CallKind::DynTrait) {
                                    1
                                } else {
                                    0
                                };
                            let next_fnptr = cur_fnptr
                                + if matches!(edge_kind, crate::callgraph::types::CallKind::FnPtr) {
                                    1
                                } else {
                                    0
                                };
                            let next_genlen = cur_genlen + edge_genlen;
                            // Update the best path if a shorter one is found
                            dist.insert(
                                *caller,
                                (
                                    next_cost,
                                    next_pkg,
                                    next_package_unique.len(),
                                    next_depth,
                                    next_dyn,
                                    next_fnptr,
                                    next_genlen,
                                ),
                            );

                            heap.push(State {
                                cost: next_cost,
                                node: *caller,
                                package_sum: next_pkg,
                                package_unique: next_package_unique,
                                depth: next_depth,
                                dyn_edges: next_dyn,
                                fnptr_edges: next_fnptr,
                                generic_args_len_sum: next_genlen,
                            });
                        }
                    }
                }
            }
        }

        // filter out the target functions
        let mut all_callers: HashMap<FunctionInstance<'tcx>, (usize, usize, usize, usize, usize, usize, usize)> =
            HashMap::new();
        for (func, (constraints, package_num, package_unique, path_len, dyn_edges, fnptr_edges, genlen_sum)) in dist {
            if !target_functions.contains(&func) {
                all_callers.insert(
                    func,
                    (
                        constraints,
                        package_num,
                        package_unique,
                        path_len,
                        dyn_edges,
                        fnptr_edges,
                        genlen_sum,
                    ),
                );
            }
        }

        let paths: Vec<PathInfo<'tcx>> = all_callers
            .into_iter()
            .filter(|(caller, _)| caller.def_id().is_local())
            .map(
                |(
                    caller,
                    (constraints, package_num, package_num_unique, path_len, dyn_edges, fnptr_edges, genlen_sum),
                )| PathInfo {
                    caller,
                    constraints,
                    package_num,
                    package_num_unique,
                    path_len,
                    dyn_edges,
                    fnptr_edges,
                    generic_args_len_sum: genlen_sum,
                },
            )
            .collect();

        if paths.is_empty() {
            tracing::warn!("No callers found for {}", target_description);
        }

        paths
    }

    /// Find all functions that directly or indirectly call the specified function
    pub fn find_callers_by_path(&self, tcx: TyCtxt<'tcx>, target_path: &str) -> Vec<PathInfo<'tcx>> {
        self.find_callers_by_predicate(tcx, &format!("path: {target_path}"), |func, tcx| {
            matches_function_path(tcx, func, target_path, self.without_args)
        })
    }
}
