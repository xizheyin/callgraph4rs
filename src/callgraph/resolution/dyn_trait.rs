use super::{
    address_taken::{def_ids_for_sig_key, sig_key},
    helpers::trivial_resolve,
};
use crate::callgraph::function::FunctionInstance;
use rustc_hir::{def, def_id::DefId};
use rustc_middle::ty::{self, TyCtxt, TypingEnv};
use std::collections::HashSet;

/// Helper to match Fn/FnMut/FnOnce traits
fn match_fn_trait<'tcx>(
    tr: &ty::ExistentialTraitRef<'tcx>,
    fn_trait: Option<DefId>,
    fn_mut_trait: Option<DefId>,
    fn_once_trait: Option<DefId>,
) -> Option<(DefId, ty::ClosureKind, ty::Ty<'tcx>)> {
    if tr.args.is_empty() {
        return None;
    }
    let args_tuple = tr.args.type_at(0);

    if Some(tr.def_id) == fn_trait {
        Some((tr.def_id, ty::ClosureKind::Fn, args_tuple))
    } else if Some(tr.def_id) == fn_mut_trait {
        Some((tr.def_id, ty::ClosureKind::FnMut, args_tuple))
    } else if Some(tr.def_id) == fn_once_trait {
        Some((tr.def_id, ty::ClosureKind::FnOnce, args_tuple))
    } else {
        None
    }
}

pub(crate) fn candidates_for_dyn_fn_trait<'tcx>(
    tcx: TyCtxt<'tcx>,
    inputs: &[ty::Ty<'tcx>],
    output: ty::Ty<'tcx>,
    address_taken_funcs: &HashSet<DefId>,
) -> Vec<FunctionInstance<'tcx>> {
    let key = sig_key(tcx, inputs, output);
    let mut candidates = Vec::new();
    for def_id in def_ids_for_sig_key(&key) {
        if !address_taken_funcs.contains(&def_id) {
            continue;
        }
        match tcx.def_kind(def_id) {
            def::DefKind::Fn | def::DefKind::AssocFn => {}
            _ => continue,
        }
        if let Some(instance) = trivial_resolve(tcx, def_id) {
            candidates.push(instance);
        } else {
            candidates.push(FunctionInstance::new_non_instance(def_id));
        }
    }
    tracing::debug!(
        "Found {} candidates for dyn fn signature with {} inputs",
        candidates.len(),
        inputs.len()
    );
    candidates
}

pub(crate) fn extract_dyn_fn_signature<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    receiver_ty: ty::Ty<'tcx>,
) -> Option<(DefId, ty::ClosureKind, Vec<ty::Ty<'tcx>>, ty::Ty<'tcx>)> {
    let preds = if let Some(ty) = peel_dyn_from_receiver(tcx, typing_env, receiver_ty)
        && let ty::TyKind::Dynamic(preds, _, _) = ty.kind()
    {
        preds
    } else {
        return None;
    };

    let li = tcx.lang_items();
    let fn_trait = li.fn_trait();
    let fn_mut_trait = li.fn_mut_trait();
    let fn_once_trait = li.fn_once_trait();
    let output_assoc = li.fn_once_output()?;

    let mut trait_def_id: Option<DefId> = None;
    let mut requested_kind: Option<ty::ClosureKind> = None;
    let mut maybe_args_tuple: Option<ty::Ty<'tcx>> = None;
    let mut maybe_output: Option<ty::Ty<'tcx>> = None;

    for pred in preds.iter() {
        let pred = tcx.normalize_erasing_late_bound_regions(typing_env, pred);
        match pred {
            ty::ExistentialPredicate::Trait(tr) => {
                if let Some((def_id, kind, args)) = match_fn_trait(&tr, fn_trait, fn_mut_trait, fn_once_trait) {
                    trait_def_id = Some(def_id);
                    requested_kind = Some(kind);
                    maybe_args_tuple = Some(args);
                }
            }
            ty::ExistentialPredicate::Projection(proj) => {
                if Some(proj.def_id) == Some(output_assoc) {
                    maybe_output = Some(proj.term.expect_type());
                }
            }
            ty::ExistentialPredicate::AutoTrait(_) => {}
        }
    }

    let (trait_id, closure_kind, args_tuple, output) =
        (trait_def_id?, requested_kind?, maybe_args_tuple?, maybe_output?);
    let inputs = match args_tuple.kind() {
        ty::TyKind::Tuple(elems) => elems.iter().collect(),
        _ => return None,
    };
    Some((trait_id, closure_kind, inputs, output))
}

pub(crate) fn extract_dyn_trait_info<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    receiver_ty: ty::Ty<'tcx>,
    method_def_id: DefId,
) -> Option<(
    DefId,
    String,
    Option<ty::ClosureKind>,
    Vec<ty::Ty<'tcx>>,
    Option<ty::Ty<'tcx>>,
)> {
    let preds = if let Some(ty) = peel_dyn_from_receiver(tcx, typing_env, receiver_ty)
        && let ty::TyKind::Dynamic(preds, _, _) = ty.kind()
    {
        preds
    } else {
        return None;
    };

    let li = tcx.lang_items();
    let fn_trait = li.fn_trait();
    let fn_mut_trait = li.fn_mut_trait();
    let fn_once_trait = li.fn_once_trait();
    let output_assoc = li.fn_once_output();

    let mut trait_def_id: Option<DefId> = None;
    let mut method_name: Option<String> = None;
    let mut requested_kind: Option<ty::ClosureKind> = None;
    let mut maybe_args_tuple: Option<ty::Ty<'tcx>> = None;
    let mut maybe_output: Option<ty::Ty<'tcx>> = None;

    for pred in preds.iter() {
        let pred = tcx.normalize_erasing_late_bound_regions(typing_env, pred);
        match pred {
            ty::ExistentialPredicate::Trait(tr) => {
                if let Some(fn_id) = fn_trait
                    && tr.def_id == fn_id
                    && tr.args.len() == 1
                {
                    trait_def_id = Some(fn_id);
                    method_name = Some("call".to_string());
                    requested_kind = Some(ty::ClosureKind::Fn);
                    maybe_args_tuple = Some(tr.args.type_at(0));
                }
                if let Some(fn_mut_id) = fn_mut_trait
                    && tr.def_id == fn_mut_id
                    && tr.args.len() == 1
                {
                    trait_def_id = Some(fn_mut_id);
                    method_name = Some("call_mut".to_string());
                    requested_kind = Some(ty::ClosureKind::FnMut);
                    maybe_args_tuple = Some(tr.args.type_at(0));
                }
                if let Some(fn_once_id) = fn_once_trait
                    && tr.def_id == fn_once_id
                    && tr.args.len() == 1
                {
                    trait_def_id = Some(fn_once_id);
                    method_name = Some("call_once".to_string());
                    requested_kind = Some(ty::ClosureKind::FnOnce);
                    maybe_args_tuple = Some(tr.args.type_at(0));
                }

                if trait_def_id.is_none() {
                    trait_def_id = Some(tr.def_id);
                    method_name = Some(tcx.item_name(method_def_id).to_string());
                }
            }
            ty::ExistentialPredicate::Projection(proj) => {
                if let Some(out_id) = output_assoc
                    && proj.def_id == out_id
                {
                    maybe_output = Some(proj.term.expect_type());
                }
            }
            ty::ExistentialPredicate::AutoTrait(_) => {}
        }
    }

    let trait_id = trait_def_id?;
    let method_name = method_name?;
    let inputs = if let Some(args_tuple) = maybe_args_tuple {
        match args_tuple.kind() {
            ty::TyKind::Tuple(elems) => elems.iter().collect(),
            _ => return None,
        }
    } else {
        Vec::new()
    };

    Some((trait_id, method_name, requested_kind, inputs, maybe_output))
}

pub(crate) fn candidates_for_dyn_normal_trait<'tcx>(
    tcx: TyCtxt<'tcx>,
    tr_id: DefId,
    method_name: &str,
) -> Vec<FunctionInstance<'tcx>> {
    let mut candidates = Vec::new();

    let trait_method_def_id = tcx.associated_item_def_ids(tr_id).iter().find_map(|&item_def_id| {
        let item = tcx.associated_item(item_def_id);
        if item.name().to_string() == method_name && matches!(item.kind, ty::AssocKind::Fn { .. }) {
            Some(item_def_id)
        } else {
            None
        }
    });

    for impl_def_id in tcx.all_impls(tr_id) {
        let mut found_override = false;
        for &item_def_id in tcx.associated_item_def_ids(impl_def_id) {
            let item = tcx.associated_item(item_def_id);
            if item.name().to_string() == method_name && matches!(item.kind, ty::AssocKind::Fn { .. }) {
                found_override = true;
                if let Some(instance) = trivial_resolve(tcx, item_def_id) {
                    candidates.push(instance);
                } else {
                    candidates.push(FunctionInstance::new_non_instance(item_def_id));
                }
            }
        }

        if !found_override && let Some(def_id) = trait_method_def_id {
            if let Some(instance) = trivial_resolve(tcx, def_id) {
                candidates.push(instance);
            } else {
                candidates.push(FunctionInstance::new_non_instance(def_id));
            }
        }
    }

    tracing::debug!(
        "Found {} candidates for dyn trait {} method {}",
        candidates.len(),
        tcx.def_path_str(tr_id),
        method_name
    );
    candidates
}

pub(crate) fn peel_dyn_from_receiver<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    mut ty: ty::Ty<'tcx>,
) -> Option<ty::Ty<'tcx>> {
    // First, peel simple wrappers iteratively
    loop {
        match ty.kind() {
            ty::TyKind::Dynamic(..) => return Some(ty),
            ty::TyKind::Ref(_, inner, _) | ty::TyKind::RawPtr(inner, _) => ty = *inner,
            _ if ty.is_box_global(tcx) => ty = ty.expect_boxed_ty(),
            _ => break,
        }
    }

    // Handle complex types
    match ty.kind() {
        ty::TyKind::Adt(adt_def, args) => {
            // Check type arguments
            for arg in args.iter() {
                if let Some(inner) = arg.as_type() {
                    if let Some(dyn_ty) = peel_dyn_from_receiver(tcx, typing_env, inner) {
                        return Some(dyn_ty);
                    }
                }
            }
            // Check transparent struct fields
            if adt_def.repr().transparent() && !adt_def.is_union() {
                let variant = adt_def.non_enum_variant();
                for field in &variant.fields {
                    let field_ty = field.ty(tcx, args);
                    if let Some(dyn_ty) = peel_dyn_from_receiver(tcx, typing_env, field_ty) {
                        return Some(dyn_ty);
                    }
                }
            }
            None
        }
        _ => {
            let tail = tcx.struct_tail_for_codegen(ty, typing_env);
            matches!(tail.kind(), ty::TyKind::Dynamic(..)).then_some(tail)
        }
    }
}
