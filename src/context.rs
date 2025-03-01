use rustc_hash::FxHashMap;
use rustc_hir::{def::DefKind, def_id::DefId};
use rustc_middle::ty::TyCtxt;
use rustc_span::Symbol;

#[derive(Clone)]
pub struct Context<'tcx> {
    /// Rust编译器的核心结构！！！
    pub tcx: TyCtxt<'tcx>,

    /// mir中DefId和名字的映射
    pub all_generic_funcs_did_sym_map: FxHashMap<DefId, Symbol>,
}

impl<'tcx> Context<'tcx> {
    /// 构造上下文
    pub fn new(tcx: TyCtxt<'tcx>, _args: FxHashMap<String, String>) -> Self {
        let mut all_generic_funcs_did_sym_map = FxHashMap::default();
        for local_def_id in tcx.hir().body_owners() {
            let did = local_def_id.to_def_id();
            match tcx.def_kind(did) {
                DefKind::Fn | DefKind::AssocFn => {
                    let name = tcx.item_name(did);
                    if !all_generic_funcs_did_sym_map.contains_key(&did) {
                        all_generic_funcs_did_sym_map.insert(did, name);
                    }
                }
                _ => {}
            }
        }

        Self {
            tcx,

            all_generic_funcs_did_sym_map,
        }
    }
}
