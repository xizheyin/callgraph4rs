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
pub enum FunctionInstance<'tcx> {
    Instance(ty::Instance<'tcx>),
    NonInstance(DefId),
}

impl<'tcx> FunctionInstance<'tcx> {
    fn new_instance(instance: ty::Instance<'tcx>) -> Self {
        Self::Instance(instance)
    }

    fn new_non_instance(def_id: DefId) -> Self {
        Self::NonInstance(def_id)
    }

    fn instance(&self) -> Option<ty::Instance<'tcx>> {
        match self {
            Self::Instance(instance) => Some(instance.clone()),
            Self::NonInstance(_) => None,
        }
    }

    fn _non_instance(&self) -> Option<DefId> {
        match self {
            Self::Instance(_) => None,
            Self::NonInstance(def_id) => Some(*def_id),
        }
    }

    fn def_id(&self) -> DefId {
        match self {
            Self::Instance(instance) => instance.def_id(),
            Self::NonInstance(def_id) => *def_id,
        }
    }

    fn is_instance(&self) -> bool {
        match self {
            Self::Instance(_) => true,
            Self::NonInstance(_) => false,
        }
    }
    fn is_non_instance(&self) -> bool {
        !self.is_instance()
    }

    fn collect_callsites(&self, tcx: ty::TyCtxt<'tcx>) -> Vec<CallSite<'tcx>> {
        let def_id = self.def_id();

        if self.is_non_instance() {
            tracing::warn!("skip non-instance function: {:?}", self);
            return Vec::new();
        }

        if !tcx.is_mir_available(def_id) {
            tracing::warn!("skip nobody function: {:?}", def_id);
            return Vec::new();
        }

        self.extract_function_call(tcx, &def_id)
    }

    /// Extract information about all function calls in `function`
    fn extract_function_call(
        &self,
        tcx: ty::TyCtxt<'tcx>,
        caller_id: &DefId,
    ) -> Vec<CallSite<'tcx>> {
        use mir::visit::Visitor;

        #[derive(Clone)]
        struct SearchFunctionCall<'tcx, 'local> {
            tcx: ty::TyCtxt<'tcx>,
            caller_instance: &'local FunctionInstance<'tcx>,
            caller_body: &'local mir::Body<'tcx>,
            callees: Vec<CallSite<'tcx>>,
        }

        impl<'tcx, 'local> SearchFunctionCall<'tcx, 'local> {
            fn new(
                tcx: ty::TyCtxt<'tcx>,
                caller_instance: &'local FunctionInstance<'tcx>,
                caller_body: &'local mir::Body<'tcx>,
            ) -> Self {
                SearchFunctionCall {
                    tcx,
                    caller_instance,
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
                    tracing::debug!(
                        "Found Call => callee: {:?}, func.ty: {:?}",
                        func,
                        func.ty(self.caller_body, self.tcx)
                    );

                    use mir::Operand::*;

                    let before_mono_ty = func.ty(self.caller_body, self.tcx);
                    let monod_result = monomorphize(
                        self.tcx,
                        self.caller_instance.instance().expect("instance is None"),
                        before_mono_ty,
                    );

                    let callee = if let Err(err) = monod_result {
                        tracing::warn!("monomorphize error: {:?}", err);
                        match func {
                            Constant(_) => match before_mono_ty.kind() {
                                ty::TyKind::FnDef(def_id, _) => {
                                    Some(FunctionInstance::new_non_instance(*def_id))
                                }
                                _ => None,
                            },
                            Move(_) | Copy(_) => {
                                tracing::warn!("skip move or copy: {:?}", func);
                                None
                            }
                        }
                    } else {
                        let monod_ty = monod_result.unwrap();

                        match func {
                            Constant(_) => match monod_ty.kind() {
                                ty::TyKind::FnDef(def_id, monoed_args) => {
                                    match self.tcx.def_kind(def_id) {
                                        def::DefKind::Fn | def::DefKind::AssocFn => {
                                            let instance_result = ty::Instance::try_resolve(
                                                self.tcx,
                                                ParamEnv::reveal_all(),
                                                *def_id,
                                                monoed_args,
                                            );

                                            match instance_result {
                                                Err(err) => {
                                                    tracing::debug!(
                                                        "Instance [{:?}] resolution error: {:?}",
                                                        def_id,
                                                        err
                                                    );
                                                    None
                                                }
                                                Ok(opt_instance) => {
                                                    if let Some(instance) = opt_instance {
                                                        tracing::debug!("instance: {:?}", instance);
                                                        Some(FunctionInstance::new_instance(
                                                            instance,
                                                        ))
                                                    } else {
                                                        tracing::debug!(
                                                        "Instance [{:?}] resolution returned None",
                                                        def_id
                                                    );
                                                        trivial_resolve(self.tcx, *def_id).or_else(|| {
                                                            tracing::warn!("Trivial resolve [{:?}] also failed, using non-instance", def_id);
                                                            Some(FunctionInstance::new_non_instance(*def_id))
                                                        })
                                                    }
                                                }
                                            }
                                        }
                                        other => {
                                            tracing::error!(
                                                "internal error: unknown call type: {:?}",
                                                other
                                            );
                                            None
                                        }
                                    }
                                }
                                _ => {
                                    tracing::error!(
                                        "internal error: unexpected function type: {:?}",
                                        monod_ty
                                    );
                                    None
                                }
                            },
                            // 移动或复制操作数
                            Move(_) | Copy(_) => {
                                tracing::warn!("skip move or copy: {:?}", func);
                                None
                            }
                        }
                    };

                    // 如果找到被调用函数，添加到调用列表
                    if let Some(callee) = callee {
                        self.callees.push(CallSite {
                            _caller: self.caller_instance.clone(),
                            callee,
                        });
                    }
                }
            }
        }

        let caller_body = tcx.optimized_mir(caller_id);
        let mut search_callees = SearchFunctionCall::new(tcx, self, caller_body);
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
                instances.push(FunctionInstance::new_instance(instance));
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
            Some(FunctionInstance::new_instance(instance))
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
        //println!("visit instance: {:?}", instance);
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
