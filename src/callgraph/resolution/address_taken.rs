use super::super::function::{FunctionInstance, iterate_all_functions};
use lazy_static::lazy_static;
use rustc_hir::{def, def_id::DefId};
use rustc_middle::{
    mir::{self, Terminator, TerminatorKind, visit::Visitor},
    ty::{self, TyCtxt, TypingEnv},
};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

struct AddressTakenCollector<'tcx> {
    #[allow(dead_code)]
    tcx: TyCtxt<'tcx>,
    address_taken: HashSet<DefId>,
}

impl<'tcx> Visitor<'tcx> for AddressTakenCollector<'tcx> {
    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: mir::Location) {
        if let TerminatorKind::Call { func, args, .. } = &terminator.kind {
            match func {
                mir::Operand::Constant(c) => {
                    if !matches!(c.ty().kind(), ty::TyKind::FnDef(..)) {
                        self.visit_operand(func, location);
                    }
                }
                _ => self.visit_operand(func, location),
            }

            for arg in args {
                self.visit_operand(&arg.node, location);
            }
            return;
        }
        self.super_terminator(terminator, location);
    }

    fn visit_operand(&mut self, operand: &mir::Operand<'tcx>, _location: mir::Location) {
        if let mir::Operand::Constant(c) = operand
            && let ty::TyKind::FnDef(def_id, _) = c.ty().kind()
        {
            self.address_taken.insert(*def_id);
        }
    }
}

lazy_static! {
    static ref FN_SIG_INDEX: Mutex<HashMap<(usize, String), Vec<DefId>>> = Mutex::new(HashMap::new());
    static ref FN_ARITY_INDEX: Mutex<HashMap<usize, Vec<DefId>>> = Mutex::new(HashMap::new());
}

pub(crate) fn collect_address_taken_functions<'tcx>(tcx: TyCtxt<'tcx>) -> HashSet<DefId> {
    let mut collector = AddressTakenCollector {
        tcx,
        address_taken: HashSet::new(),
    };

    iterate_all_functions(
        tcx,
        |_| true,
        |def_id| {
            if tcx.is_mir_available(def_id) {
                let body = tcx.optimized_mir(def_id);
                collector.visit_body(body);
            }
            None::<FunctionInstance>
        },
    );

    tracing::info!("Found {} address-taken functions", collector.address_taken.len());
    collector.address_taken
}

pub(crate) fn build_fn_sig_index<'tcx>(tcx: TyCtxt<'tcx>, address_taken_funcs: &HashSet<DefId>) {
    let mut map: HashMap<(usize, String), Vec<DefId>> = HashMap::new();
    let mut arity_map: HashMap<usize, Vec<DefId>> = HashMap::new();

    for def_id in address_taken_funcs.iter().copied() {
        match tcx.def_kind(def_id) {
            def::DefKind::Fn | def::DefKind::AssocFn => {}
            _ => continue,
        }

        let env = TypingEnv::post_analysis(tcx, def_id);
        let sig = tcx.normalize_erasing_late_bound_regions(env, tcx.fn_sig(def_id).skip_binder());
        let key = sig_key(tcx, sig.inputs(), sig.output());
        map.entry(key).or_default().push(def_id);
        arity_map.entry(sig.inputs().len()).or_default().push(def_id);
    }

    *FN_SIG_INDEX.lock().unwrap() = map;
    *FN_ARITY_INDEX.lock().unwrap() = arity_map;
}

/// Generate a normalized signature key for function indexing.
/// Used internally by fnptr resolution to match function signatures.
pub(super) fn sig_key<'tcx>(
    tcx: TyCtxt<'tcx>,
    inputs: &[ty::Ty<'tcx>],
    output: ty::Ty<'tcx>,
) -> (usize, String) {
    let erased_out = tcx.erase_regions(output);
    let mut key = format!("{:?}", erased_out);
    for input in inputs {
        key.push('|');
        key.push_str(&format!("{:?}", tcx.erase_regions(*input)));
    }
    (inputs.len(), key)
}

/// Retrieve function DefIds matching a specific signature key.
/// Returns empty vec if no matches found.
pub(super) fn def_ids_for_sig_key(key: &(usize, String)) -> Vec<DefId> {
    FN_SIG_INDEX.lock().unwrap().get(key).cloned().unwrap_or_default()
}

/// Retrieve function DefIds with a specific arity (parameter count).
/// Used for loose signature matching when exact match fails.
pub(super) fn def_ids_for_arity(arity: usize) -> Vec<DefId> {
    FN_ARITY_INDEX.lock().unwrap().get(&arity).cloned().unwrap_or_default()
}
