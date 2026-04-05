use super::super::function::FunctionInstance;
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir,
    ty::{self, Instance, TyCtxt, TypeFoldable, TypingEnv},
};

/// Monomorphize a value in the context of an instance
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

/// Trivially resolve a DefId to a FunctionInstance
///
/// # Arguments
/// * `tcx` - Type context
/// * `def_id` - Definition ID to resolve
///
/// # Returns
/// * `Option<FunctionInstance<'_>>` - FunctionInstance if resolved, None otherwise
pub(crate) fn trivial_resolve(tcx: TyCtxt<'_>, def_id: DefId) -> Option<FunctionInstance<'_>> {
    let ty = tcx.type_of(def_id).skip_binder();
    if let ty::TyKind::FnDef(def_id, args) = ty.kind() {
        let instance = ty::Instance::try_resolve(
            tcx,
            TypingEnv::post_analysis(tcx, def_id),
            *def_id,
            args,
        );
        if let Ok(Some(instance)) = instance {
            Some(FunctionInstance::new_instance(instance))
        } else {
            None
        }
    } else {
        None
    }
}

/// Extract callable DefId from a type, recursively unwrapping references and pointers
pub(crate) fn fallback_callable_def_id_from_ty<'tcx>(
    tcx: TyCtxt<'tcx>,
    ty: ty::Ty<'tcx>,
) -> Option<DefId> {
    match ty.kind() {
        ty::TyKind::FnDef(def_id, _) => Some(*def_id),
        ty::TyKind::Closure(def_id, _) => Some(*def_id),
        ty::TyKind::Ref(_, inner, _) => fallback_callable_def_id_from_ty(tcx, *inner),
        ty::TyKind::RawPtr(inner, _) => fallback_callable_def_id_from_ty(tcx, *inner),
        _ if ty.is_box_global(tcx) => fallback_callable_def_id_from_ty(tcx, ty.expect_boxed_ty()),
        _ => None,
    }
}

/// Extract function definition from an operand
pub(crate) fn operand_fn_def<'tcx>(
    operand: &mir::Operand<'tcx>,
) -> Option<(DefId, ty::GenericArgsRef<'tcx>)> {
    match operand {
        mir::Operand::Constant(c) => match c.ty().kind() {
            ty::TyKind::FnDef(def_id, args) => Some((*def_id, *args)),
            _ => operand.const_fn_def(),
        },
        _ => operand.const_fn_def(),
    }
}

/// Convert DefIds to FunctionInstances, filtering by address-taken set
pub(crate) fn def_ids_to_instances<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_ids: &[DefId],
    address_taken_funcs: &std::collections::HashSet<DefId>,
) -> Vec<FunctionInstance<'tcx>> {
    def_ids
        .iter()
        .filter(|id| address_taken_funcs.contains(id))
        .filter_map(|id| trivial_resolve(tcx, *id))
        .collect()
}
