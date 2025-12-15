use crate::args::CGArgs;
use crate::callgraph::function::FunctionInstance;
use crate::callgraph::types::CallGraph;
use rustc_middle::ty::TyCtxt;
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::Write;

pub fn analyze_safety_abstractions<'tcx>(call_graph: &CallGraph<'tcx>, tcx: TyCtxt<'tcx>, args: &CGArgs) {
    tracing::info!("Starting RQ2: Safety Abstractions Analysis");

    let mut unsafe_targets = HashSet::new();
    let mut safe_callers = HashSet::new();
    let mut call_map: HashMap<_, Vec<_>> = HashMap::new();
    let mut reverse_call_map: HashMap<_, Vec<_>> = HashMap::new();

    // 1. Identify Unsafe Targets and Safe Callers
    // Build call maps for traversal
    let target_patterns = &args.find_callers;
    let has_targets = !target_patterns.is_empty();

    for call_site in &call_graph.call_sites {
        let caller = call_site.caller();
        let callee = call_site.callee();

        let caller_def_id = caller.def_id();
        let callee_def_id = callee.def_id();

        // Check if callee is unsafe
        let callee_safety = if tcx.is_closure_like(callee_def_id) {
            if let Some(inst) = callee.instance() {
                inst.args.as_closure().sig().safety()
            } else {
                rustc_hir::Safety::Safe
            }
        } else {
            tcx.fn_sig(callee_def_id).skip_binder().safety()
        };

        if callee_safety.is_unsafe() {
            // If targets are specified, only include them
            if !has_targets
                || target_patterns
                    .iter()
                    .any(|p| crate::callgraph::utils::matches_function_path(tcx, callee, p, args.without_args))
            {
                unsafe_targets.insert(callee);
            }
        }

        // Check if caller is safe
        let caller_safety = if tcx.is_closure_like(caller_def_id) {
            if let Some(inst) = caller.instance() {
                inst.args.as_closure().sig().safety()
            } else {
                rustc_hir::Safety::Safe
            }
        } else {
            tcx.fn_sig(caller_def_id).skip_binder().safety()
        };

        if caller_safety.is_safe() {
            safe_callers.insert(caller);
        }

        call_map.entry(caller).or_default().push(callee);
        reverse_call_map.entry(callee).or_default().push(caller);
    }

    let unsafe_target_count = unsafe_targets.len();
    let total_functions = call_graph.total_functions;

    // Metric 1: Unsafe Sourcing
    // Proportion of unsafe target functions
    let unsafe_ratio = if total_functions > 0 {
        unsafe_target_count as f64 / total_functions as f64
    } else {
        0.0
    };

    tracing::info!("RQ2 Metric 1: Unsafe Sourcing");
    tracing::info!("  Total Functions: {}", total_functions);
    tracing::info!("  Unsafe Targets: {}", unsafe_target_count);
    tracing::info!("  Unsafe Ratio: {:.2}%", unsafe_ratio * 100.0);

    // Metric 2 & 3: Safety Boundary Penetration & Encapsulation Depth
    let mut exposed_unsafe_functions = 0;
    let mut total_boundaries_found = 0;
    let mut total_encapsulation_depth = 0;
    let mut max_encapsulation_depth = 0;

    // Metric 4: Propagation Statistics
    let mut prop_total_path_len = 0;
    let mut prop_max_path_len = 0;
    let mut prop_min_path_len = usize::MAX;
    let mut prop_path_count = 0;
    let mut prop_rooted_count = 0;

    // Store detailed info for JSON output
    let mut exposed_details = Vec::new();

    for &unsafe_target in &unsafe_targets {
        // --- Metric 2 & 3: BFS to find ALL safe callers (Encapsulation) ---
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut is_target_exposed = false;

        queue.push_back((unsafe_target, 0));
        visited.insert(unsafe_target);

        while let Some((current_func, depth)) = queue.pop_front() {
            if safe_callers.contains(&current_func) {
                // Found a safe caller!
                if depth > 0 {
                    total_boundaries_found += 1;
                    total_encapsulation_depth += depth;
                    if depth > max_encapsulation_depth {
                        max_encapsulation_depth = depth;
                    }
                    is_target_exposed = true;

                    exposed_details.push(json!({
                        "unsafe_function": unsafe_target.full_path(tcx, false),
                        "safe_caller": current_func.full_path(tcx, false),
                        "depth": depth
                    }));

                    // Continue to find other boundaries, but don't extend this safe path
                    continue;
                }
            }

            if let Some(callers) = reverse_call_map.get(&current_func) {
                for &caller in callers {
                    if !visited.contains(&caller) {
                        visited.insert(caller);
                        queue.push_back((caller, depth + 1));
                    }
                }
            }
        }

        if is_target_exposed {
            exposed_unsafe_functions += 1;
        }

        // --- Metric 4: DFS to find full propagation paths (Statistics) ---
        // We trace paths from the unsafe function upwards to finding roots or reaching a limit
        // Limit max paths per unsafe function to avoid exponential explosion
        let max_paths_per_func = 20;
        let max_depth = 15;
        let paths = find_paths_to_root(unsafe_target, &reverse_call_map, max_depth, max_paths_per_func);

        for path in paths {
            let len = path.len(); // Length in nodes (call depth = len - 1)
            if len > 1 {
                // Ignore trivial paths if any
                prop_path_count += 1;
                prop_total_path_len += len;
                if len > prop_max_path_len {
                    prop_max_path_len = len;
                }
                if len < prop_min_path_len {
                    prop_min_path_len = len;
                }

                // Check if rooted: the last node has no callers in the map
                let last_node = path.last().unwrap();
                if !reverse_call_map.contains_key(last_node) || reverse_call_map[last_node].is_empty() {
                    prop_rooted_count += 1;
                }
            }
        }
    }

    // Calculations for Metric 2 & 3
    let avg_encapsulation_depth = if total_boundaries_found > 0 {
        total_encapsulation_depth as f64 / total_boundaries_found as f64
    } else {
        0.0
    };

    // Calculations for Metric 4
    let avg_prop_path_length = if prop_path_count > 0 {
        prop_total_path_len as f64 / prop_path_count as f64
    } else {
        0.0
    };
    let rooted_ratio = if prop_path_count > 0 {
        prop_rooted_count as f64 / prop_path_count as f64
    } else {
        0.0
    };
    // Reset min if no paths found
    if prop_path_count == 0 {
        prop_min_path_len = 0;
    }

    tracing::info!("RQ2 Metric 2: Safety Boundary Penetration");
    tracing::info!("  Exposed Unsafe Functions: {}", exposed_unsafe_functions);
    tracing::info!(
        "  Exposure Rate: {:.2}%",
        if unsafe_target_count > 0 {
            exposed_unsafe_functions as f64 / unsafe_target_count as f64 * 100.0
        } else {
            0.0
        }
    );

    tracing::info!("RQ2 Metric 3: Encapsulation Depth");
    tracing::info!("  Average Depth: {:.2}", avg_encapsulation_depth);
    tracing::info!("  Max Depth: {}", max_encapsulation_depth);

    tracing::info!("RQ2 Metric 4: Propagation Statistics");
    tracing::info!("  Paths Analyzed: {}", prop_path_count);
    tracing::info!("  Avg Path Length: {:.2}", avg_prop_path_length);
    tracing::info!("  Rooted Ratio: {:.2}%", rooted_ratio * 100.0);

    // Output JSON result
    let result = json!({
        "crate_name": tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).to_string(),
        "rq2_safety_abstractions": {
            "unsafe_sourcing": {
                "total_functions": total_functions,
                "unsafe_targets": unsafe_target_count,
                "unsafe_ratio": unsafe_ratio
            },
            "safety_boundary": {
                "exposed_unsafe_functions": exposed_unsafe_functions,
                "exposure_rate": if unsafe_target_count > 0 { exposed_unsafe_functions as f64 / unsafe_target_count as f64 } else { 0.0 }
            },
            "encapsulation_depth": {
                "average_depth": avg_encapsulation_depth,
                "max_depth": max_encapsulation_depth
            },
            "propagation_statistics": {
                "total_paths_analyzed": prop_path_count,
                "average_path_length": avg_prop_path_length,
                "max_path_length": prop_max_path_len,
                "min_path_length": prop_min_path_len,
                "rooted_paths_count": prop_rooted_count,
                "rooted_ratio": rooted_ratio
            },
            "exposed_details": exposed_details
        }
    });

    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("./target"));

    if !output_dir.exists() {
        let _ = fs::create_dir_all(&output_dir);
    }

    let crate_name = tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).to_string();
    let output_path = output_dir.join(format!("{}-rq2-safety.json", crate_name));

    match File::create(&output_path) {
        Ok(mut file) => {
            if let Err(e) = write!(file, "{}", serde_json::to_string_pretty(&result).unwrap()) {
                tracing::error!("Failed to write RQ2 results to file: {}", e);
            } else {
                tracing::info!("RQ2 results written to {}", output_path.display());
            }
        }
        Err(e) => {
            tracing::error!("Failed to create RQ2 output file: {}", e);
        }
    }
}

/// Helper to find paths from a node to roots in the reverse call graph
fn find_paths_to_root<'tcx>(
    start: FunctionInstance<'tcx>,
    reverse_map: &HashMap<FunctionInstance<'tcx>, Vec<FunctionInstance<'tcx>>>,
    depth_limit: usize,
    max_paths: usize,
) -> Vec<Vec<FunctionInstance<'tcx>>> {
    let mut results = Vec::new();
    let mut stack = vec![(start, vec![start])];

    // Using iterative DFS to avoid recursion depth issues
    while let Some((current, path)) = stack.pop() {
        // Check constraints
        if results.len() >= max_paths {
            break;
        }

        // Check depth limit
        if path.len() >= depth_limit {
            results.push(path);
            continue;
        }

        // Get callers (reverse edges)
        if let Some(callers) = reverse_map.get(&current) {
            if callers.is_empty() {
                // No callers -> Root
                results.push(path);
            } else {
                let mut extended = false;
                for &caller in callers {
                    // Cycle detection
                    if !path.contains(&caller) {
                        let mut new_path = path.clone();
                        new_path.push(caller);
                        stack.push((caller, new_path));
                        extended = true;
                    }
                }
                // If we couldn't extend (all callers were cycles), treat as end of path
                if !extended {
                    results.push(path);
                }
            }
        } else {
            // No entry in map -> Root
            results.push(path);
        }
    }

    results
}
