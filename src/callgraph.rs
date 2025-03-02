use rustc_hir::{def, def_id::DefId};
use rustc_middle::{
    mir::{self, Terminator, TerminatorKind},
    ty::{self, ParamEnv},
};
use std::collections::VecDeque;

use crate::mono;

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

#[derive(Debug, Clone, Copy)]
pub struct FunctionInstance<'tcx> {
    instance: ty::Instance<'tcx>,
}

impl<'tcx> FunctionInstance<'tcx> {
    fn new(instance: ty::Instance<'tcx>) -> Self {
        Self { instance }
    }

    fn collect_callsites(
        &self,
        tcx: ty::TyCtxt<'tcx>,
        substs: &mono::Monomorphizer<'tcx>,
    ) -> Vec<CallSite<'tcx>> {
        let def_id = self.instance.def.def_id();
        if !def_id.is_local() && !tcx.is_mir_available(def_id) {
            println!("skip external function: {:?}", def_id);
            return Vec::new();
        }
        let instance = self.instance;
        let mir = tcx.optimized_mir(def_id);
        self.extract_function_call(tcx, mir, &instance.def.def_id(), substs)
    }

    /// Extract information about all function calls in `function`
    fn extract_function_call(
        &self,
        tcx: ty::TyCtxt<'tcx>,
        function: &mir::Body<'tcx>,
        caller_id: &DefId,
        substs: &mono::Monomorphizer<'tcx>,
    ) -> Vec<CallSite<'tcx>> {
        use mir::visit::Visitor;

        #[derive(Clone)]
        struct SearchFunctionCall<'tcx, 'local> {
            tcx: ty::TyCtxt<'tcx>,
            caller_instance: &'local FunctionInstance<'tcx>,
            caller_id: &'local DefId,
            callees: Vec<CallSite<'tcx>>,
            substs: &'local mono::Monomorphizer<'tcx>,
        }

        impl<'tcx, 'local> SearchFunctionCall<'tcx, 'local> {
            fn new(
                tcx: ty::TyCtxt<'tcx>,
                caller_instance: &'local FunctionInstance<'tcx>,
                caller_id: &'local DefId,
                substs: &'local mono::Monomorphizer<'tcx>,
            ) -> Self {
                SearchFunctionCall {
                    tcx,
                    caller_instance,
                    caller_id,
                    callees: Vec::default(),
                    substs,
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

                    let callee = match func {
                        Constant(cst) => {
                            if let ty::TyKind::FnDef(def_id, args) = cst.const_.ty().kind() {
                                let monoed_args = self.substs.mono_arguments(args);
                                let def_id = *def_id;
                                let param_env = self.tcx.param_env(self.caller_id);
                                match self.tcx.def_kind(def_id) {
                                    def::DefKind::Fn | def::DefKind::AssocFn => {
                                        // Check if this is a trait method call and resolve to the actual implementation

                                        if let Ok(Some(callee_instance)) = ty::Instance::try_resolve(
                                            self.tcx,
                                            param_env,
                                            def_id,
                                            monoed_args,
                                        ) {
                                            Some(FunctionInstance::new(callee_instance))
                                        } else {
                                            trivial_resolve(self.tcx, def_id)
                                        }
                                    }
                                    other => {
                                        panic!("internal error: unknown call type: {:?}", other);
                                    }
                                }
                            } else {
                                panic!("internal error: unknown call type: {:?}", cst);
                            }
                        }
                        Move(_place) | Copy(_place) => todo!(),
                    };
                    if let Some(callee) = callee {
                        self.callees.push(CallSite {
                            _caller: self.caller_instance.clone(),
                            callee,
                        });
                    }
                }
            }
        }

        let mut search_callees = SearchFunctionCall::new(tcx, self, caller_id, substs);
        search_callees.visit_body(function);
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

    while let Some(instance) = call_graph.instances.pop_front() {
        let substs = mono::Monomorphizer::new(tcx, instance.instance.args.to_vec());
        let call_sites = instance.collect_callsites(tcx, &substs);
        for call_site in call_sites {
            println!("call_site: {:?}", call_site);
            call_graph.instances.push_back(call_site.callee);
            call_graph.call_sites.push(call_site);
        }
    }
    call_graph
}
