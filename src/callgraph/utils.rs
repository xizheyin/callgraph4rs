use std::collections::{HashMap, HashSet, VecDeque};

use rustc_hir::def_id::DefId;
use rustc_middle::ty::TyCtxt;

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
        if potential_version
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
        {
            return potential_version.to_string();
        }
    }

    // If we can't find a proper version, use the crate hash as a unique identifier
    format!("0.0.0-{}", crate_hash.to_string().split_at(8).0)
}

impl<'tcx> CallGraph<'tcx> {
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

        tracing::info!(
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
    ) -> Option<Vec<PathInfo<'tcx>>>
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
            return None;
        }

        tracing::info!(
            "Found {} functions matching {}",
            target_functions.len(),
            target_description
        );
        for func in &target_functions {
            tracing::info!(
                "Matched function: {}",
                self.function_instance_to_string(tcx, *func)
            );
        }

        // Create mapping from callee to callers with constraint counts
        let mut callee_to_callers: HashMap<
            FunctionInstance<'tcx>,
            HashMap<FunctionInstance<'tcx>, (usize, usize)>,
        > = HashMap::new();

        for call_site in &self.call_sites {
            let caller = call_site.caller();
            let callee = call_site.callee();
            let constraints = call_site.constraint_count();
            let package_num = call_site.package_num();

            callee_to_callers
                .entry(callee)
                .or_default()
                .entry(caller)
                .and_modify(|(c, p)| {
                    *c += constraints;
                    *p += package_num;
                })
                .or_insert((constraints, package_num));
        }

        // Find all direct and indirect callers with constraint counts
        let mut all_callers: HashMap<FunctionInstance<'tcx>, (usize, usize)> = HashMap::new();
        let mut queue: VecDeque<(FunctionInstance<'tcx>, (usize, usize))> =
            target_functions.into_iter().map(|f| (f, (0, 0))).collect();
        let mut processed: HashSet<FunctionInstance<'tcx>> = HashSet::new();

        while let Some((current, (path_constraints, path_package_num))) = queue.pop_front() {
            if processed.contains(&current) {
                continue;
            }
            processed.insert(current);

            if let Some(callers) = callee_to_callers.get(&current) {
                for (caller, (constraints, package_num)) in callers {
                    // 累计约束数：当前路径约束 + 当前调用约束
                    let total_constraints = path_constraints + constraints;
                    let total_package_num = path_package_num + package_num;

                    if !processed.contains(caller) {
                        // 插入或更新调用者的约束累计值
                        all_callers
                            .entry(*caller)
                            .and_modify(|(c, p)| {
                                // 如果已存在，取较小值（最短路径）
                                if total_constraints < *c {
                                    *c = total_constraints;
                                }
                                *p = total_package_num;
                            })
                            .or_insert((total_constraints, total_package_num));

                        queue.push_back((*caller, (total_constraints, total_package_num)));
                    }
                }
            }
        }

        if all_callers.is_empty() {
            tracing::warn!("No callers found for {}", target_description);
            return None;
        }

        // 返回调用者列表及其约束数量
        Some(
            all_callers
                .into_iter()
                .map(|(caller, (constraints, package_num))| PathInfo {
                    caller,
                    constraints,
                    package_num,
                })
                .collect(),
        )
    }

    /// Find all functions that directly or indirectly call the specified function
    pub fn find_callers_by_path(
        &self,
        tcx: TyCtxt<'tcx>,
        target_path: &str,
    ) -> Option<Vec<PathInfo<'tcx>>> {
        self.find_callers_by_predicate(tcx, &format!("path: {target_path}"), |func, tcx| {
            // Get complete function path (including generic parameters)
            let full_func_path = self.function_instance_to_string(tcx, func);

            // Also get the basic path without generic parameters
            let base_path = match func {
                FunctionInstance::Instance(inst) => tcx.def_path_str(inst.def_id()),
                FunctionInstance::NonInstance(def_id) => tcx.def_path_str(def_id),
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
    }
}
