use rustc_hir::{def, def_id::DefId};
use rustc_middle::mir::{self, Terminator, TerminatorKind};
use rustc_middle::ty::{self, TypingEnv};
use std::collections::HashMap;

use super::analysis::{monomorphize, trivial_resolve};
use super::types::CallSite;
use crate::constraint_utils::BlockPath;

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

    fn instance(&self) -> Option<ty::Instance<'tcx>> {
        match self {
            Self::Instance(instance) => Some(*instance),
            Self::NonInstance(_) => None,
        }
    }

    pub(crate) fn _non_instance(&self) -> Option<DefId> {
        match self {
            Self::Instance(_) => None,
            Self::NonInstance(def_id) => Some(*def_id),
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

    pub(crate) fn collect_callsites(&self, tcx: ty::TyCtxt<'tcx>) -> Vec<CallSite<'tcx>> {
        let def_id = self.def_id();

        if self.is_non_instance() {
            tracing::warn!("skip non-instance function: {:?}", self);
            return Vec::new();
        }

        if !tcx.is_mir_available(def_id) {
            tracing::warn!("skip nobody function: {:?}", def_id);
            return Vec::new();
        }
        let constraints = super::analysis::get_constraints(tcx, def_id);
        self.extract_function_call(tcx, &def_id, constraints)
    }

    /// Extract information about all function calls in `function`
    fn extract_function_call(
        &self,
        tcx: ty::TyCtxt<'tcx>,
        caller_id: &DefId,
        constraints: HashMap<mir::BasicBlock, BlockPath>,
    ) -> Vec<CallSite<'tcx>> {
        use mir::visit::Visitor;

        #[derive(Clone)]
        struct SearchFunctionCall<'tcx, 'local> {
            tcx: ty::TyCtxt<'tcx>,
            caller_instance: &'local FunctionInstance<'tcx>,
            caller_body: &'local mir::Body<'tcx>,
            callees: Vec<CallSite<'tcx>>,
            constraints: HashMap<mir::BasicBlock, BlockPath>,
            current_bb: mir::BasicBlock,
        }

        impl<'tcx, 'local> SearchFunctionCall<'tcx, 'local> {
            fn new(
                tcx: ty::TyCtxt<'tcx>,
                caller_instance: &'local FunctionInstance<'tcx>,
                caller_body: &'local mir::Body<'tcx>,
                constraints: HashMap<mir::BasicBlock, BlockPath>,
            ) -> Self {
                SearchFunctionCall {
                    tcx,
                    caller_instance,
                    caller_body,
                    callees: Vec::default(),
                    constraints,
                    current_bb: mir::BasicBlock::from_usize(0),
                }
            }
        }

        impl<'tcx, 'local> Visitor<'tcx> for SearchFunctionCall<'tcx, 'local> {
            fn visit_basic_block_data(
                &mut self,
                block: mir::BasicBlock,
                data: &mir::BasicBlockData<'tcx>,
            ) {
                self.current_bb = block;
                self.super_basic_block_data(block, data);
            }

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
                    let typing_env =
                        TypingEnv::post_analysis(self.tcx, self.caller_instance.def_id());

                    let before_mono_ty = func.ty(self.caller_body, self.tcx);
                    let monod_result = monomorphize(
                        self.tcx,
                        typing_env,
                        self.caller_instance.instance().expect("instance is None"),
                        before_mono_ty,
                    );

                    let callee = if let Err(err) = monod_result {
                        tracing::warn!("Monomorphization failed: {:?}", err);
                        match func {
                            Constant(_) => match before_mono_ty.kind() {
                                ty::TyKind::FnDef(def_id, _) => {
                                    tracing::warn!(
                                        "Callee {:?} is not monomorphized, using non-instance",
                                        def_id
                                    );
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
                                            tracing::debug!("Try resolve instance: {:?}", monod_ty);
                                            let instance_result = ty::Instance::try_resolve(
                                                self.tcx,
                                                typing_env,
                                                *def_id,
                                                monoed_args,
                                            );

                                            match instance_result {
                                                Err(err) => {
                                                    tracing::error!(
                                                        "Instance [{:?}] resolution error: {:?}",
                                                        monod_ty,
                                                        err
                                                    );
                                                    None
                                                }
                                                Ok(opt_instance) => {
                                                    if let Some(instance) = opt_instance {
                                                        tracing::info!(
                                                            "Resolved instance successfully: {:?}",
                                                            instance
                                                        );
                                                        Some(FunctionInstance::new_instance(
                                                            instance,
                                                        ))
                                                    } else {
                                                        tracing::warn!(
                                                            "Resolve [{:#?}] failed, try trivial resolve",
                                                            monod_ty
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
                            // Move or copy operands
                            Move(_) | Copy(_) => {
                                tracing::warn!("skip move or copy: {:?}", func);
                                None
                            }
                        }
                    };

                    // If callee function is found, add to the call list
                    if let Some(callee) = callee {
                        self.callees.push(CallSite::new(
                            *self.caller_instance,
                            callee,
                            self.constraints[&self.current_bb].constraints,
                        ));
                    }
                }
            }
        }

        let caller_body = tcx.optimized_mir(caller_id);
        let mut search_callees = SearchFunctionCall::new(tcx, self, caller_body, constraints);
        search_callees.visit_body(caller_body);
        search_callees.callees
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
    instances
}
