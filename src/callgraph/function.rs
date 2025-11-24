use rustc_hir::def_id::DefId;
use rustc_middle::{
    middle::exported_symbols::ExportedSymbol,
    ty::{self, TyCtxt, TypingEnv},
};

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

    /// Convert function instance to readable string
    pub(crate) fn full_path(&self, tcx: TyCtxt<'tcx>, without_args: bool) -> String {
        match self {
            Self::Instance(inst) => {
                let def_id = inst.def_id();

                // Determine whether to include generic arguments based on the without_args option
                if !without_args && !inst.args.is_empty() {
                    // Include generic parameter information
                    tcx.def_path_str_with_args(def_id, inst.args)
                } else {
                    // Skip generic parameter information
                    tcx.def_path_str(def_id)
                }
            }
            Self::NonInstance(def_id) => {
                // For non-instances, only show the path
                tcx.def_path_str(def_id)
            }
        }
    }
}

/// collect all function instances in local crate, including generic instances
pub fn collect_local_instances(tcx: ty::TyCtxt<'_>) -> Vec<FunctionInstance<'_>> {
    let mut instances = Vec::new();
    for def_id in tcx.hir_body_owners() {
        let ty = tcx.type_of(def_id).skip_binder();
        if let ty::TyKind::FnDef(def_id, args) = ty.kind() {
            let instance = ty::Instance::try_resolve(tcx, TypingEnv::post_analysis(tcx, *def_id), *def_id, args);
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

    // Get def_id from ExportedSymbol
    let get_def_id = |sym: &ExportedSymbol| -> Option<DefId> {
        match sym {
            ExportedSymbol::NonGeneric(def_id) => Some(*def_id),
            ExportedSymbol::Generic(def_id, _) => Some(*def_id),
            ExportedSymbol::ThreadLocalShim(def_id) => Some(*def_id),
            ExportedSymbol::AsyncDropGlue(def_id, _) => Some(*def_id),
            _ => None,
        }
    };

    // 遍历本地 crate：通过 HIR body owners 筛选函数与关联函数
    {
        use rustc_hir::def::DefKind;
        for owner in tcx.hir_body_owners() {
            let def_id = owner.to_def_id();
            match tcx.def_kind(def_id) {
                DefKind::Fn | DefKind::AssocFn => {
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

    // 遍历外部 crates：使用 exported_symbols 获取可导出的函数/关联函数
    {
        use rustc_hir::def::DefKind;
        for &crate_num in tcx.crates(()) {
            let mut exported_symbols = tcx.exported_generic_symbols(crate_num).to_vec();
            exported_symbols.extend_from_slice(tcx.exported_non_generic_symbols(crate_num));
            for &(sym, _export) in &exported_symbols {
                if let Some(def_id) = get_def_id(&sym) {
                    match tcx.def_kind(def_id) {
                        DefKind::Fn | DefKind::AssocFn => {
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
