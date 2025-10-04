use std::collections::{HashMap, HashSet};

use rustc_hir::{def, def_id::DefId};
use rustc_middle::{
    mir::{self, Terminator, TerminatorKind},
    ty::{
        self, normalize_erasing_regions::NormalizationError, Instance, TyCtxt, TypeFoldable,
        TypingEnv,
    },
};
use tracing::{debug, error, info, warn};

use super::controlflow::{self, BlockPath};
use super::{function::FunctionInstance, types::CallGraph, types::CallSite};
use crate::timer;

impl<'tcx> FunctionInstance<'tcx> {
    pub(crate) fn collect_callsites(&self, tcx: ty::TyCtxt<'tcx>) -> Vec<CallSite<'tcx>> {
        let def_id = self.def_id();

        if self.is_non_instance() {
            tracing::warn!("Skip non-instance(No body) function: {:?}", self);
            return Vec::new();
        }

        if !tcx.is_mir_available(def_id) {
            tracing::warn!("Skip no-body(No mir available) function: {:?}", def_id);
            return Vec::new();
        }

        // Compute function internal constraints
        let constraints = timer::measure("compute_constraints", || get_constraints(tcx, def_id));

        // Extract function call information
        timer::measure("extract_function_call", || {
            self.extract_function_call(tcx, &def_id, constraints)
        })
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

            /// Deal with normalize error
            ///
            /// If the function is a constant function, return the non-instance function.
            /// Otherwise, return None.
            fn handle_mono_error(
                &self,
                func: &mir::Operand<'tcx>,
                before_mono_ty: ty::Ty<'tcx>,
                err: NormalizationError,
            ) -> Option<FunctionInstance<'tcx>> {
                use mir::Operand::*;
                tracing::warn!("Monomorphization failed: {:?}", err);
                match func {
                    Constant(_) => match before_mono_ty.kind() {
                        // Although the monomorphization failed, the function is a constant function
                        // So we can use DefId to construct non-instance function
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
                        tracing::warn!("skip move or copy: {:?}", before_mono_ty);
                        None
                    }
                }
            }

            /// Handle monomorphized callee
            fn handle_monoed_callee(
                &mut self,
                func: &mir::Operand<'tcx>,
                monod_ty: ty::Ty<'tcx>,
            ) -> Option<FunctionInstance<'tcx>> {
                use mir::Operand::*;
                match func {
                    Constant(_) => self.handle_monoed_direct_callee(monod_ty),
                    // Move or copy operands - 支持函数指针调用
                    Move(_) | Copy(_) => self.handle_monoed_indirect_callee(monod_ty),
                }
            }

            /// Handle monomorphized direct callee
            fn handle_monoed_direct_callee(
                &mut self,
                monod: ty::Ty<'tcx>,
            ) -> Option<FunctionInstance<'tcx>> {
                if let ty::TyKind::FnDef(def_id, monoed_args) = monod.kind() {
                    // FnDef represents a direct function call
                    // such as `let x = func(y);`
                    match self.tcx.def_kind(def_id) {
                        // bare function, method, associated function
                        def::DefKind::Fn | def::DefKind::AssocFn => {
                            debug!("Try resolve instance: {:?}", monod);
                            let instance_result = ty::Instance::try_resolve(
                                self.tcx,
                                TypingEnv::post_analysis(self.tcx, *def_id), // Use callee-specific typing environment for resolution
                                *def_id,
                                monoed_args,
                            );

                            match instance_result {
                                Err(err) => {
                                    error!("Instance [{:?}] resolve failed: {:?}", monod, err)
                                }
                                Ok(opt_instance) => {
                                    if let Some(instance) = opt_instance {
                                        info!("Resolved instance successfully: {:?}", instance);
                                        return Some(FunctionInstance::new_instance(instance));
                                    } else {
                                        warn!("Resolve [{:#?}] failed, trivial resolve", monod);
                                        return trivial_resolve(self.tcx, *def_id).or_else(|| {
                                                            warn!("Trivial resolve [{:?}] also failed, using non-instance", def_id);
                                                            Some(FunctionInstance::new_non_instance(*def_id))
                                                        });
                                    }
                                }
                            }
                        }
                        other => error!("unknown callee type: {:?}", other),
                    }
                } else {
                    error!("unexpected function type: {:?}", monod.kind());
                }
                None
            }

            /// Handle monomorphized indirect callee
            ///
            /// When Operand is Move/Copy, the callee is a function pointer
            /// We need to resolve the function pointer to get the actual callee
            fn handle_monoed_indirect_callee(
                &mut self,
                monod_ty: ty::Ty<'tcx>,
            ) -> Option<FunctionInstance<'tcx>> {
                match monod_ty.kind() {
                    ty::TyKind::FnPtr(poly_sig, _) => {
                        let sig = poly_sig.skip_binder();
                        let candidates = candidates_for_fnptr_sig(self.tcx, sig);
                        if candidates.is_empty() {
                            tracing::warn!(
                                "fnptr call: no candidates found for signature {:?}",
                                poly_sig
                            );
                        } else {
                            tracing::info!(
                                "fnptr call: found {} signature-matched candidates",
                                candidates.len()
                            );
                            for cand in candidates {
                                self.callees.push(CallSite::new(
                                    *self.caller_instance,
                                    cand,
                                    self.constraints[&self.current_bb].constraints,
                                ));
                            }
                        }
                        // We have already inserted the candidates into the call list
                        // So we just return None here
                        None
                    }
                    _ => {
                        tracing::warn!("skip move or copy (unsupported type): {:?}", monod_ty);
                        None
                    }
                }
            }
        }

        impl<'tcx, 'local> Visitor<'tcx> for SearchFunctionCall<'tcx, 'local> {
            fn visit_basic_block_data(
                &mut self,
                block: mir::BasicBlock,
                data: &mir::BasicBlockData<'tcx>,
            ) {
                // Update current basic block
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

                    let typing_env =
                        TypingEnv::post_analysis(self.tcx, self.caller_instance.def_id());

                    let before_mono_ty = func.ty(self.caller_body, self.tcx);

                    // Perform monomorphization
                    let monod_result = monomorphize(
                        self.tcx,
                        typing_env,
                        self.caller_instance.instance().expect("instance is None"),
                        before_mono_ty,
                    );

                    let callee = match monod_result {
                        Ok(monoed) => self.handle_monoed_callee(func, monoed),
                        Err(err) => self.handle_mono_error(func, before_mono_ty, err),
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

/// Trivial resolve function instance from def_id
///
/// This function is used to resolve function instance from def_id
/// without monomorphization.
///
/// # Returns
/// * `Option<FunctionInstance<'_>>` - FunctionInstance if resolved, None otherwise
pub(crate) fn trivial_resolve(tcx: ty::TyCtxt<'_>, def_id: DefId) -> Option<FunctionInstance<'_>> {
    let ty = tcx.type_of(def_id).skip_binder();
    if let ty::TyKind::FnDef(def_id, args) = ty.kind() {
        let instance =
            ty::Instance::try_resolve(tcx, TypingEnv::post_analysis(tcx, def_id), *def_id, args);
        if let Ok(Some(instance)) = instance {
            Some(FunctionInstance::new_instance(instance))
        } else {
            None
        }
    } else {
        None
    }
}

// Perform monomorphization while constructing call graph
pub(crate) fn perform_mono_analysis<'tcx>(
    tcx: ty::TyCtxt<'tcx>,
    instances: Vec<FunctionInstance<'tcx>>,
    args: &crate::args::CGArgs,
) -> CallGraph<'tcx> {
    let mut call_graph = CallGraph::new(instances, args.without_args);
    let mut discovered = HashSet::new();

    while let Some(instance) = call_graph.instances.pop_front() {
        let call_sites = timer::measure("instance_callsites", || instance.collect_callsites(tcx));

        for call_site in call_sites {
            call_graph.call_sites.push(call_site.clone());
            if discovered.contains(&call_site.callee()) {
                continue;
            }
            discovered.insert(call_site.callee());
            call_graph.instances.push_back(call_site.callee());
        }
    }

    tracing::info!(
        "Analysis complete: {} instances analyzed, {} call sites found",
        discovered.len(),
        call_graph.call_sites.len(),
    );

    // Deduplicate call sites if deduplication is not disabled
    if !args.no_dedup {
        tracing::info!("Deduplication enabled - removing duplicate call sites");
        timer::measure("deduplicate_call_sites", || {
            call_graph.deduplicate_call_sites()
        });
    } else {
        tracing::info!("Deduplication disabled - keeping all call sites");
    }

    call_graph
}

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

/// Get internal constraints from the body of a function
pub(crate) fn get_constraints(
    tcx: ty::TyCtxt,
    def_id: DefId,
) -> HashMap<mir::BasicBlock, BlockPath> {
    let mir = tcx.optimized_mir(def_id);
    controlflow::compute_shortest_paths(mir)
}

/// Find function candidates that match the given function pointer signature
pub(crate) fn candidates_for_fnptr_sig<'tcx>(
    tcx: TyCtxt<'tcx>,
    sig: ty::FnSigTys<TyCtxt<'tcx>>,
) -> Vec<FunctionInstance<'tcx>> {
    fn types_match_ignoring_regions<'tcx>(
        tcx: TyCtxt<'tcx>,
        a: ty::Ty<'tcx>,
        b: ty::Ty<'tcx>,
    ) -> bool {
        // 擦除 regions 后比较类型
        let a_erased = tcx.erase_regions(a);
        let b_erased = tcx.erase_regions(b);
        a_erased == b_erased
    }

    // Cache borrowed inputs/output once to avoid accidental moves
    let sig_inputs = sig.inputs();
    let sig_output = sig.output();

    let mut candidates = Vec::new();

    for owner_def_id in tcx.hir_body_owners() {
        let ty = tcx.type_of(owner_def_id).skip_binder();
        if let ty::TyKind::FnDef(fn_def_id, _args) = ty.kind() {
            let candidate_sig = tcx.fn_sig(*fn_def_id).skip_binder();

            if candidate_sig.inputs().skip_binder().len() == sig_inputs.len() {
                let return_types_match = types_match_ignoring_regions(
                    tcx,
                    candidate_sig.output().skip_binder(),
                    sig_output,
                );

                if return_types_match {
                    let cand_inputs = candidate_sig.inputs().skip_binder();
                    let inputs_match =
                        cand_inputs
                            .iter()
                            .zip(sig_inputs.iter())
                            .all(|(cand_input, sig_input)| {
                                types_match_ignoring_regions(tcx, *cand_input, *sig_input)
                            });

                    if inputs_match {
                        if let Some(instance) = trivial_resolve(tcx, *fn_def_id) {
                            candidates.push(instance);
                        } else {
                            candidates.push(FunctionInstance::new_non_instance(*fn_def_id));
                        }
                    }
                }
            }
        }
    }

    tracing::debug!(
        "Found {} candidates for fnptr signature with {} inputs",
        candidates.len(),
        sig_inputs.len()
    );
    candidates
}
