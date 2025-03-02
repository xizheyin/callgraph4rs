use rustc_hir::def_id::DefId;
use rustc_middle::ty::{GenericArgKind, GenericArgsRef, Ty, TyCtxt, TyKind};

use crate::mono::Monomorphizer;

/// Returns false if any of the generic arguments are themselves generic
pub fn are_concrete(generic_args: GenericArgsRef<'_>) -> bool {
    for gen_arg in generic_args.iter() {
        if let GenericArgKind::Type(ty) = gen_arg.unpack() {
            if !is_concrete(ty.kind()) {
                return false;
            }
        }
    }
    true
}

/// Determines if the given type is fully concrete.
pub fn is_concrete(ty_kind: &TyKind<'_>) -> bool {
    match ty_kind {
        TyKind::Adt(_, gen_args)
        | TyKind::Closure(_, gen_args)
        | TyKind::FnDef(_, gen_args)
        | TyKind::Coroutine(_, gen_args)
        | TyKind::CoroutineWitness(_, gen_args)
        | TyKind::Alias(_, rustc_middle::ty::AliasTy { args: gen_args, .. }) => {
            are_concrete(gen_args)
        }
        TyKind::Tuple(types) => types.iter().all(|t| is_concrete(t.kind())),
        TyKind::Bound(..)
        | TyKind::Dynamic(..)
        | TyKind::Error(..)
        | TyKind::Infer(..)
        | TyKind::Param(..) => false,
        TyKind::Ref(_, ty, _) => is_concrete(ty.kind()),
        _ => true,
    }
}

pub fn is_fn_once_output(tcx: TyCtxt<'_>, id: DefId) -> bool {
    let items = tcx.lang_items();
    matches!(Some(id), x if x == items.fn_once_output())
}

pub fn is_fn_once_call_once(_tcx: TyCtxt<'_>, _id: DefId) -> bool {
    false
}

pub fn function_return_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
    gen_args: GenericArgsRef<'tcx>,
) -> Ty<'tcx> {
    let fn_sig = tcx.fn_sig(def_id);
    let ret_type = fn_sig.skip_binder().output().skip_binder();
    let generic_args = gen_args.to_vec();
    let monomorphizer = Monomorphizer::new(tcx, generic_args);
    monomorphizer.mono_type(ret_type)
}

pub fn closure_return_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    _def_id: DefId,
    gen_args: GenericArgsRef<'tcx>,
) -> Ty<'tcx> {
    let fn_sig = gen_args.as_closure().sig();
    let ret_type = fn_sig.skip_binder().output();
    let generic_args = gen_args.to_vec();
    let monomorphizer = Monomorphizer::new(tcx, generic_args);
    monomorphizer.mono_type(ret_type)
}
