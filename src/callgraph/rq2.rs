use crate::args::CGArgs;
use crate::callgraph::function::FunctionInstance;
use crate::callgraph::types::CallGraph;
use rustc_middle::ty::TyCtxt;
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::Write;

pub fn analyze_public_exposure<'tcx>(call_graph: &CallGraph<'tcx>, tcx: TyCtxt<'tcx>, args: &CGArgs) {
    tracing::info!("Starting RQ2: Public Exposure Analysis");

    let mut targets = HashSet::new();

    let local_instances = crate::callgraph::function::collect_local_instances(tcx);
    let mut public_entries: HashSet<FunctionInstance<'tcx>> = HashSet::new();

    let mut call_map: HashMap<_, Vec<_>> = HashMap::new();
    let mut reverse_call_map: HashMap<_, Vec<_>> = HashMap::new();

    let target_patterns = &args.find_callers;
    let has_targets = !target_patterns.is_empty();

    for f in &local_instances {
        if tcx.visibility(f.def_id()).is_public() {
            public_entries.insert(*f);
        }
    }

    for call_site in &call_graph.call_sites {
        let caller = call_site.caller();
        let callee = call_site.callee();

        if has_targets {
            if target_patterns.iter().any(|p| {
                crate::callgraph::utils::matches_function_path(tcx, callee, p, args.without_args)
            }) {
                targets.insert(callee);
            }
            if target_patterns.iter().any(|p| {
                crate::callgraph::utils::matches_function_path(tcx, caller, p, args.without_args)
            }) {
                targets.insert(caller);
            }
        }


        call_map.entry(caller).or_default().push(callee);
        reverse_call_map.entry(callee).or_default().push(caller);
    }

    if !has_targets {
        tracing::warn!("RQ2 public exposure: no targets specified (find_callers empty)");
    }

    let mut discovered = HashSet::new();
    let mut queue: VecDeque<FunctionInstance<'tcx>> = local_instances.into_iter().collect();
    for f in queue.iter() {
        discovered.insert(*f);
    }
    while let Some(f) = queue.pop_front() {
        if let Some(callees) = call_map.get(&f) {
            for &callee in callees {
                if discovered.insert(callee) {
                    queue.push_back(callee);
                }
            }
        }
    }

    if has_targets {
        for &f in &discovered {
            if target_patterns.iter().any(|p| {
                crate::callgraph::utils::matches_function_path(tcx, f, p, args.without_args)
            }) {
                targets.insert(f);
            }
        }
    }

    fn pctl_usize(values: &mut [usize], p: f64) -> usize {
        if values.is_empty() {
            return 0;
        }
        values.sort_unstable();
        let n = values.len();
        let rank = (p * n as f64).ceil() as usize;
        let idx = rank.saturating_sub(1).min(n - 1);
        values[idx]
    }

    fn median_usize_as_f64(values: &mut [usize]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        values.sort_unstable();
        let n = values.len();
        if n % 2 == 1 {
            values[n / 2] as f64
        } else {
            (values[n / 2 - 1] as f64 + values[n / 2] as f64) / 2.0
        }
    }

    let total_targets = targets.len();
    let mut public_reachable_targets = 0usize;

    let mut public_entry_fanin_sum = 0usize;
    let mut public_entry_fanin_samples: Vec<usize> = Vec::new();
    let mut public_entry_fanin_samples_over_public: Vec<usize> = Vec::new();

    let mut min_public_distance_samples: Vec<usize> = Vec::new();
    let mut encapsulation_depth_samples: Vec<usize> = Vec::new();

    let mut public_exposure_details: Vec<serde_json::Value> = Vec::new();

    for &target in &targets {
        let mut q: VecDeque<(FunctionInstance<'tcx>, usize)> = VecDeque::new();
        let mut visited: HashSet<FunctionInstance<'tcx>> = HashSet::new();
        let mut per_entry_min_depth: HashMap<FunctionInstance<'tcx>, usize> = HashMap::new();

        q.push_back((target, 0));
        visited.insert(target);

        while let Some((cur, depth)) = q.pop_front() {
            if public_entries.contains(&cur) {
                per_entry_min_depth
                    .entry(cur)
                    .and_modify(|d| *d = (*d).min(depth))
                    .or_insert(depth);
            }
            if let Some(callers) = reverse_call_map.get(&cur) {
                for &caller in callers {
                    if visited.insert(caller) {
                        q.push_back((caller, depth + 1));
                    }
                }
            }
        }

        let public_entry_fanin = per_entry_min_depth.len();
        public_entry_fanin_sum += public_entry_fanin;
        public_entry_fanin_samples.push(public_entry_fanin);

        let min_public_distance = per_entry_min_depth.values().copied().min();
        if public_entry_fanin > 0 {
            public_reachable_targets += 1;
            public_entry_fanin_samples_over_public.push(public_entry_fanin);
            if let Some(d) = min_public_distance {
                min_public_distance_samples.push(d);
                encapsulation_depth_samples.push(d.saturating_sub(1));
            }
        }

        for (entry, d) in per_entry_min_depth.into_iter() {
            public_exposure_details.push(json!({
                "target": target.full_path(tcx, false),
                "public_entry": entry.full_path(tcx, false),
                "distance": d
            }));
        }
    }

    let public_reachability_rate = if total_targets == 0 {
        0.0
    } else {
        public_reachable_targets as f64 / total_targets as f64
    };

    let public_entry_fanin_mean = if total_targets == 0 {
        0.0
    } else {
        public_entry_fanin_sum as f64 / total_targets as f64
    };
    let public_entry_fanin_median = median_usize_as_f64(&mut public_entry_fanin_samples);
    let public_entry_fanin_p95 = pctl_usize(&mut public_entry_fanin_samples, 0.95);

    let public_entry_fanin_mean_over_public_targets =
        if public_entry_fanin_samples_over_public.is_empty() {
            0.0
        } else {
            public_entry_fanin_samples_over_public.iter().sum::<usize>() as f64
                / public_entry_fanin_samples_over_public.len() as f64
        };

    let min_public_distance_mean_over_public_targets = if min_public_distance_samples.is_empty() {
        0.0
    } else {
        min_public_distance_samples.iter().sum::<usize>() as f64
            / min_public_distance_samples.len() as f64
    };
    let min_public_distance_median_over_public_targets =
        median_usize_as_f64(&mut min_public_distance_samples);
    let min_public_distance_p95_over_public_targets =
        pctl_usize(&mut min_public_distance_samples, 0.95);

    let encapsulation_depth_mean_over_public_targets = if encapsulation_depth_samples.is_empty() {
        0.0
    } else {
        encapsulation_depth_samples.iter().sum::<usize>() as f64
            / encapsulation_depth_samples.len() as f64
    };
    let encapsulation_depth_median_over_public_targets =
        median_usize_as_f64(&mut encapsulation_depth_samples);
    let encapsulation_depth_p95_over_public_targets =
        pctl_usize(&mut encapsulation_depth_samples, 0.95);

    public_exposure_details.sort_by(|a, b| {
        let da = a.get("distance").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
        let db = b.get("distance").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
        da.cmp(&db)
    });
    let public_exposure_details = public_exposure_details.into_iter().take(200).collect::<Vec<_>>();

    let result = json!({
        "crate_name": tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).to_string(),
        "rq2_public_exposure": {
            "total_targets": total_targets,
            "public_reachable_targets": public_reachable_targets,
            "public_reachability_rate": public_reachability_rate,
            "public_entry_fanin_mean": public_entry_fanin_mean,
            "public_entry_fanin_median": public_entry_fanin_median,
            "public_entry_fanin_p95": public_entry_fanin_p95,
            "public_entry_fanin_mean_over_public_targets": public_entry_fanin_mean_over_public_targets,
            "min_public_distance_mean_over_public_targets": min_public_distance_mean_over_public_targets,
            "min_public_distance_median_over_public_targets": min_public_distance_median_over_public_targets,
            "min_public_distance_p95_over_public_targets": min_public_distance_p95_over_public_targets,
            "encapsulation_depth_mean_over_public_targets": encapsulation_depth_mean_over_public_targets,
            "encapsulation_depth_median_over_public_targets": encapsulation_depth_median_over_public_targets,
            "encapsulation_depth_p95_over_public_targets": encapsulation_depth_p95_over_public_targets
        },
        "public_exposure_details": public_exposure_details
    });

    let output_dir = args
        .output_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("./target"));

    if !output_dir.exists() {
        let _ = fs::create_dir_all(&output_dir);
    }

    let crate_name = tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).to_string();
    let output_path = output_dir.join(format!("{}-rq2-public-exposure.json", crate_name));

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

