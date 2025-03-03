use rustc_hir::{def, def_id::DefId};
use rustc_middle::ty::{Instance, TyCtxt, TypeFoldable};
use rustc_middle::{
    mir::{self, Terminator, TerminatorKind},
    ty::{self, ParamEnv},
};
use std::collections::{HashSet, VecDeque};

pub(crate) struct CallGraph<'tcx> {
    _all_generic_instances: Vec<FunctionInstance<'tcx>>,
    instances: VecDeque<FunctionInstance<'tcx>>,
    pub call_sites: Vec<CallSite<'tcx>>,
}

impl<'tcx> CallGraph<'tcx> {
    fn new(all_generic_instances: Vec<FunctionInstance<'tcx>>) -> Self {
        Self {
            _all_generic_instances: all_generic_instances.clone(),
            instances: all_generic_instances.into_iter().collect(),
            call_sites: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallSite<'tcx> {
    _caller: FunctionInstance<'tcx>,
    callee: FunctionInstance<'tcx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionInstance<'tcx> {
    instance: ty::Instance<'tcx>,
}

impl<'tcx> FunctionInstance<'tcx> {
    fn new(instance: ty::Instance<'tcx>) -> Self {
        Self { instance }
    }

    fn collect_callsites(&self, tcx: ty::TyCtxt<'tcx>) -> Vec<CallSite<'tcx>> {
        let def_id = self.instance.def.def_id();
        if !def_id.is_local() && !tcx.is_mir_available(def_id) {
            println!("skip external function: {:?}", def_id);
            return Vec::new();
        }
        let instance = self.instance;
        let mir = tcx.optimized_mir(def_id);
        self.extract_function_call(tcx, mir, &instance.def.def_id())
    }

    /// Extract information about all function calls in `function`
    fn extract_function_call(
        &self,
        tcx: ty::TyCtxt<'tcx>,
        caller_body: &mir::Body<'tcx>,
        caller_id: &DefId,
    ) -> Vec<CallSite<'tcx>> {
        use mir::visit::Visitor;

        #[derive(Clone)]
        struct SearchFunctionCall<'tcx, 'local> {
            tcx: ty::TyCtxt<'tcx>,
            caller_instance: &'local FunctionInstance<'tcx>,
            caller_body: &'local mir::Body<'tcx>,
            caller_id: &'local DefId,
            callees: Vec<CallSite<'tcx>>,
        }

        impl<'tcx, 'local> SearchFunctionCall<'tcx, 'local> {
            fn new(
                tcx: ty::TyCtxt<'tcx>,
                caller_instance: &'local FunctionInstance<'tcx>,
                caller_id: &'local DefId,
                caller_body: &'local mir::Body<'tcx>,
            ) -> Self {
                SearchFunctionCall {
                    tcx,
                    caller_instance,
                    caller_id,
                    caller_body,
                    callees: Vec::default(),
                }
            }
        }

        impl<'tcx, 'local> Visitor<'tcx> for SearchFunctionCall<'tcx, 'local> {
            fn visit_terminator(
                &mut self,
                terminator: &Terminator<'tcx>,
                _location: mir::Location,
            ) {
                if let TerminatorKind::Call { func, .. } = &terminator.kind {
                    use mir::Operand::*;
                    let monod_callee_func_ty = monomorphize(
                        self.tcx,
                        self.caller_instance.instance,
                        func.ty(self.caller_body, self.tcx),
                    );
                    let callee = monod_callee_func_ty.ok().and_then(|monod_ty| match func {
                        Constant(_) => match monod_ty.kind() {
                            ty::TyKind::FnDef(def_id, monoed_args) => {
                                match self.tcx.def_kind(def_id) {
                                    def::DefKind::Fn | def::DefKind::AssocFn => {
                                        ty::Instance::try_resolve(
                                            self.tcx,
                                            ParamEnv::reveal_all(),
                                            *def_id,
                                            monoed_args,
                                        )
                                        .ok()
                                        .flatten()
                                        .map(FunctionInstance::new)
                                        .or_else(|| trivial_resolve(self.tcx, *def_id))
                                    }
                                    other => {
                                        panic!("internal error: unknown call type: {:?}", other);
                                    }
                                }
                            }
                            _ => panic!("internal error: unexpected function type: {:?}", monod_ty),
                        },
                        Move(_) | Copy(_) => todo!(),
                    });
                    if let Some(callee) = callee {
                        self.callees.push(CallSite {
                            _caller: self.caller_instance.clone(),
                            callee,
                        });
                    }
                }
            }
        }

        let mut search_callees = SearchFunctionCall::new(tcx, self, caller_id, caller_body);
        search_callees.visit_body(caller_body);
        search_callees.callees
    }
}

pub fn collect_generic_instances(tcx: ty::TyCtxt<'_>) -> Vec<FunctionInstance<'_>> {
    let mut instances = Vec::new();
    for def_id in tcx.hir().body_owners() {
        let ty = tcx.type_of(def_id).skip_binder();
        if let ty::TyKind::FnDef(def_id, args) = ty.kind() {
            let instance = ty::Instance::try_resolve(tcx, ParamEnv::empty(), *def_id, args);
            if let Ok(Some(instance)) = instance {
                instances.push(FunctionInstance::new(instance));
            }
        }
    }
    instances
}

fn trivial_resolve(tcx: ty::TyCtxt<'_>, def_id: DefId) -> Option<FunctionInstance<'_>> {
    let ty = tcx.type_of(def_id).skip_binder();
    if let ty::TyKind::FnDef(def_id, args) = ty.kind() {
        let instance = ty::Instance::try_resolve(tcx, ParamEnv::empty(), *def_id, args);
        if let Ok(Some(instance)) = instance {
            Some(FunctionInstance::new(instance))
        } else {
            None
        }
    } else {
        None
    }
}

pub fn perform_mono_analysis<'tcx>(
    tcx: ty::TyCtxt<'tcx>,
    instances: Vec<FunctionInstance<'tcx>>,
) -> CallGraph<'tcx> {
    let mut call_graph = CallGraph::new(instances);
    let mut visited = HashSet::new();

    while let Some(instance) = call_graph.instances.pop_front() {
        if visited.contains(&instance) {
            continue;
        }
        visited.insert(instance);
        let call_sites = instance.collect_callsites(tcx);
        for call_site in call_sites {
            //println!("call_site: {:?}", call_site);
            call_graph.instances.push_back(call_site.callee);
            call_graph.call_sites.push(call_site);
        }
    }
    call_graph
}

pub fn monomorphize<'tcx, T>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    value: T,
) -> Result<T, ty::normalize_erasing_regions::NormalizationError<'tcx>>
where
    T: TypeFoldable<TyCtxt<'tcx>>,
{
    instance.try_instantiate_mir_and_normalize_erasing_regions(
        tcx,
        ty::ParamEnv::reveal_all(),
        ty::EarlyBinder::bind(value),
    )
}
