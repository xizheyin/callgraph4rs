use rustc_hir::def_id::DefId;
use rustc_middle::ty::{self, TypingEnv};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionInstance<'tcx> {
    Instance(ty::Instance<'tcx>),
    NonInstance(DefId),
}

impl<'tcx> FunctionInstance<'tcx> {
    pub(crate) fn new_instance(instance: ty::Instance<'tcx>) -> Self {
        Self::Instance(instance)
    }

    pub(crate) fn new_non_instance(def_id: DefId) -> Self {
        Self::NonInstance(def_id)
    }

    pub(crate) fn instance(&self) -> Option<ty::Instance<'tcx>> {
        match self {
            Self::Instance(instance) => Some(*instance),
            Self::NonInstance(_) => None,
        }
    }

    pub(crate) fn def_id(&self) -> DefId {
        match self {
            Self::Instance(instance) => instance.def_id(),
            Self::NonInstance(def_id) => *def_id,
        }
    }

    pub(crate) fn is_instance(&self) -> bool {
        match self {
            Self::Instance(_) => true,
            Self::NonInstance(_) => false,
        }
    }
    pub(crate) fn is_non_instance(&self) -> bool {
        !self.is_instance()
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

    // Log the number of instances found
    tracing::info!("Collected {} generic instances", instances.len());

    instances
}
