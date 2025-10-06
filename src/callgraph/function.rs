use rustc_hir::def_id::{DefId, LOCAL_CRATE};
use rustc_middle::ty::{self, TyCtxt, TypingEnv};

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

/// collect all function instances in local crate, including generic instances
pub fn collect_local_instances(tcx: ty::TyCtxt<'_>) -> Vec<FunctionInstance<'_>> {
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

/// iterates all functions in all crates, including local crate
/// filter is used to filter out some functions
/// processor is used to process each function
pub(crate) fn iterate_all_functions<'tcx, F, P>(
    tcx: TyCtxt<'tcx>,
    mut filter: F,
    mut processor: P,
) -> Vec<FunctionInstance<'tcx>>
where
    F: FnMut(DefId) -> bool,
    P: FnMut(DefId) -> Option<FunctionInstance<'tcx>>,
{
    let mut results = Vec::new();
    // all crates, including local crate
    let all_crates = tcx.crates(()).iter().chain(std::iter::once(&LOCAL_CRATE));
    for &crate_num in all_crates {
        let root = crate_num.as_def_id(); // root def id of the crate
        let mut queue = std::collections::VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        queue.push_back(root);

        while let Some(mod_id) = queue.pop_front() {
            if !seen.insert(mod_id) {
                continue;
            }
            for child in tcx.module_children(mod_id) {
                if let Some(def_id) = child.res.opt_def_id() {
                    use rustc_hir::def;
                    match tcx.def_kind(def_id) {
                        def::DefKind::Mod => {
                            queue.push_back(def_id);
                        }
                        def::DefKind::Fn | def::DefKind::AssocFn => {
                            if filter(def_id) {
                                if let Some(instance) = processor(def_id) {
                                    results.push(instance);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    results
}
