use super::{
    address_taken::{def_ids_for_arity, def_ids_for_sig_key, sig_key},
    helpers::{def_ids_to_instances, trivial_resolve},
};
use crate::callgraph::function::FunctionInstance;
use rustc_hir::def_id::DefId;
use rustc_middle::ty::{self, Binder, TyCtxt, TypingEnv};
use std::collections::{HashMap, HashSet};

pub(crate) fn candidates_for_fnptr_sig<'tcx>(
    tcx: TyCtxt<'tcx>,
    caller_def_id: DefId,
    sig: Binder<'tcx, ty::FnSigTys<TyCtxt<'tcx>>>,
    address_taken_funcs: &HashSet<DefId>,
) -> Vec<FunctionInstance<'tcx>> {
    let sig = tcx.normalize_erasing_late_bound_regions(TypingEnv::post_analysis(tcx, caller_def_id), sig);
    let sig_inputs = sig.inputs();
    let sig_output = sig.output();
    let key = sig_key(tcx, sig_inputs, sig_output);
    let mut candidates = def_ids_to_instances(tcx, &def_ids_for_sig_key(&key), address_taken_funcs);
    if candidates.is_empty() {
        candidates = candidates_for_fnptr_sig_loose(
            tcx,
            caller_def_id,
            sig_inputs,
            sig_output,
            address_taken_funcs,
        );
    }
    tracing::debug!(
        "Found {} candidates for fnptr signature with {} inputs",
        candidates.len(),
        sig_inputs.len()
    );
    candidates
}

fn candidates_for_fnptr_sig_loose<'tcx>(
    tcx: TyCtxt<'tcx>,
    _caller_def_id: DefId,
    actual_inputs: &[ty::Ty<'tcx>],
    actual_output: ty::Ty<'tcx>,
    address_taken_funcs: &HashSet<DefId>,
) -> Vec<FunctionInstance<'tcx>> {
    let mut matched = Vec::new();
    for def_id in def_ids_for_arity(actual_inputs.len()) {
        if !address_taken_funcs.contains(&def_id) {
            continue;
        }
        let env = TypingEnv::post_analysis(tcx, def_id);
        let cand_sig = tcx.normalize_erasing_late_bound_regions(env, tcx.fn_sig(def_id).skip_binder());
        if !fn_sig_loosely_matches(tcx, actual_inputs, actual_output, cand_sig.inputs(), cand_sig.output()) {
            continue;
        }
        if let Some(instance) = trivial_resolve(tcx, def_id) {
            matched.push(instance);
        } else {
            matched.push(FunctionInstance::new_non_instance(def_id));
        }
    }

    tracing::debug!(
        "Loose fnptr signature fallback found {} candidates with {} inputs",
        matched.len(),
        actual_inputs.len()
    );
    matched
}

fn fn_sig_loosely_matches<'tcx>(
    tcx: TyCtxt<'tcx>,
    actual_inputs: &[ty::Ty<'tcx>],
    actual_output: ty::Ty<'tcx>,
    cand_inputs: &[ty::Ty<'tcx>],
    cand_output: ty::Ty<'tcx>,
) -> bool {
    if actual_inputs.len() != cand_inputs.len() {
        return false;
    }

    let mut param_bindings: HashMap<u32, ty::Ty<'tcx>> = HashMap::new();
    for (actual, cand) in actual_inputs.iter().zip(cand_inputs.iter()) {
        if !loosely_match_ty(tcx, *actual, *cand, &mut param_bindings) {
            return false;
        }
    }
    loosely_match_ty(tcx, actual_output, cand_output, &mut param_bindings)
}

fn loosely_match_ty<'tcx>(
    tcx: TyCtxt<'tcx>,
    actual: ty::Ty<'tcx>,
    candidate: ty::Ty<'tcx>,
    param_bindings: &mut HashMap<u32, ty::Ty<'tcx>>,
) -> bool {
    let actual = tcx.erase_regions(actual);
    let candidate = tcx.erase_regions(candidate);

    if actual == candidate {
        return true;
    }

    if matches!(candidate.kind(), ty::TyKind::Alias(..)) || matches!(actual.kind(), ty::TyKind::Alias(..)) {
        return true;
    }

    match candidate.kind() {
        ty::TyKind::Param(param) => match param_bindings.get(&param.index) {
            Some(bound) => *bound == actual,
            None => {
                param_bindings.insert(param.index, actual);
                true
            }
        },
        ty::TyKind::Ref(_, cand_inner, cand_mut) => match actual.kind() {
            ty::TyKind::Ref(_, actual_inner, actual_mut) => {
                cand_mut == actual_mut && loosely_match_ty(tcx, *actual_inner, *cand_inner, param_bindings)
            }
            _ => false,
        },
        ty::TyKind::RawPtr(cand_inner, cand_mut) => match actual.kind() {
            ty::TyKind::RawPtr(actual_inner, actual_mut) => {
                cand_mut == actual_mut && loosely_match_ty(tcx, *actual_inner, *cand_inner, param_bindings)
            }
            _ => false,
        },
        ty::TyKind::Adt(cand_def, cand_args) => match actual.kind() {
            ty::TyKind::Adt(actual_def, actual_args) => {
                if cand_def.did() != actual_def.did() || cand_args.len() != actual_args.len() {
                    return false;
                }
                for (cand_arg, actual_arg) in cand_args.iter().zip(actual_args.iter()) {
                    match (cand_arg.as_type(), actual_arg.as_type()) {
                        (Some(cand_ty), Some(actual_ty)) => {
                            if !loosely_match_ty(tcx, actual_ty, cand_ty, param_bindings) {
                                return false;
                            }
                        }
                        _ if cand_arg == actual_arg => {}
                        _ => return false,
                    }
                }
                true
            }
            _ => false,
        },
        ty::TyKind::Tuple(cand_elems) => match actual.kind() {
            ty::TyKind::Tuple(actual_elems) => {
                if cand_elems.len() != actual_elems.len() {
                    return false;
                }
                cand_elems
                    .iter()
                    .zip(actual_elems.iter())
                    .all(|(cand_ty, actual_ty)| loosely_match_ty(tcx, actual_ty, cand_ty, param_bindings))
            }
            _ => false,
        },
        ty::TyKind::Slice(cand_inner) => match actual.kind() {
            ty::TyKind::Slice(actual_inner) => loosely_match_ty(tcx, *actual_inner, *cand_inner, param_bindings),
            _ => false,
        },
        ty::TyKind::Array(cand_inner, cand_len) => match actual.kind() {
            ty::TyKind::Array(actual_inner, actual_len) => {
                cand_len == actual_len && loosely_match_ty(tcx, *actual_inner, *cand_inner, param_bindings)
            }
            _ => false,
        },
        _ => false,
    }
}
