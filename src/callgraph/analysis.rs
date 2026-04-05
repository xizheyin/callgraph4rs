//! In Rust, there are two types of function calls:
//! 1. Static dispatch: the function to call is determined at compile time.
//! 2. Dynamic dispatch: the function to call is determined at runtime.

use super::{
    controlflow::{BlockPath, compute_shortest_paths},
    function::FunctionInstance,
    origin::OriginTraceContext,
    resolution::{
        build_fn_sig_index, candidates_for_dyn_fn_trait, candidates_for_dyn_normal_trait, candidates_for_fnptr_sig,
        collect_address_taken_functions, extract_dyn_fn_signature, extract_dyn_trait_info,
        fallback_callable_def_id_from_ty, monomorphize, operand_fn_def, peel_dyn_from_receiver, trivial_resolve,
    },
    types::{CallGraph, CallKind, CallSite},
};
use crate::timer;

use rustc_hir::{def, def_id::DefId};
use rustc_middle::{
    mir::{self, Terminator, TerminatorKind, visit::Visitor},
    ty::{self, InstanceKind, TypingEnv, normalize_erasing_regions::NormalizationError},
};
use rustc_span::source_map::Spanned;
use std::collections::{HashMap, HashSet};
use tracing::{debug, error, warn};

impl<'tcx> FunctionInstance<'tcx> {
    /// the entrypoint to collect all callsites in a function instance
    pub(crate) fn collect_callsites(
        &self,
        tcx: ty::TyCtxt<'tcx>,
        address_taken_funcs: &HashSet<DefId>,
    ) -> Vec<CallSite<'tcx>> {
        let def_id = self.def_id();

        if self.is_non_instance() {
            tracing::warn!("Skip non-instance(No body) function: {:?}", self);
            return Vec::new();
        }

        if !tcx.is_mir_available(def_id) {
            tracing::warn!("Skip no-body(No mir available) function: {:?}", def_id);
            return Vec::new();
        }

        // Compute function internal constraints,
        // which is a mapping from basic block to the path from the entry block to the basic block.
        let constraints = timer::measure("1.0.0compute_constraints", || compute_shortest_paths(tcx, def_id));

        // Extract function call information
        timer::measure("1.0.1extract_function_call", || {
            self.extract_function_call(tcx, &def_id, constraints, address_taken_funcs)
        })
    }

    /// Extract information about all function calls in `function`
    fn extract_function_call(
        &self,
        tcx: ty::TyCtxt<'tcx>,
        caller_id: &DefId,
        constraints: HashMap<mir::BasicBlock, BlockPath>,
        address_taken_funcs: &HashSet<DefId>,
    ) -> Vec<CallSite<'tcx>> {
        let caller_body = tcx.optimized_mir(caller_id);
        let mut search_callees = SearchFunctionCall::new(tcx, self, caller_body, constraints, address_taken_funcs);
        search_callees.visit_body(caller_body);
        search_callees.callees
    }
}

#[derive(Clone)]
struct SearchFunctionCall<'tcx, 'local> {
    tcx: ty::TyCtxt<'tcx>,
    caller_instance: &'local FunctionInstance<'tcx>,
    caller_body: &'local mir::Body<'tcx>,
    callees: Vec<CallSite<'tcx>>,
    constraints: HashMap<mir::BasicBlock, BlockPath>,
    current_bb: mir::BasicBlock,
    address_taken_funcs: &'local HashSet<DefId>,
    typing_env: TypingEnv<'tcx>,
}

impl<'tcx, 'local> Visitor<'tcx> for SearchFunctionCall<'tcx, 'local> {
    fn visit_basic_block_data(&mut self, block: mir::BasicBlock, data: &mir::BasicBlockData<'tcx>) {
        // Update current basic block
        self.current_bb = block;
        self.super_basic_block_data(block, data);
    }

    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, _location: mir::Location) {
        if let TerminatorKind::Call { func, args, .. } | TerminatorKind::TailCall { func, args, .. } = &terminator.kind
        {
            let func_ty = func.ty(self.caller_body, self.tcx);
            tracing::debug!("Found callee: {:?}, func.ty: {:?}", func, func_ty);

            let typing_env = self.typing_env;
            let before_mono_ty = func.ty(self.caller_body, self.tcx);

            // Perform monomorphization
            let monod_result = timer::measure("1.0.1.0monomorphize", || {
                monomorphize(
                    self.tcx,
                    typing_env,
                    self.caller_instance.instance().expect("instance is None"),
                    before_mono_ty,
                )
            });

            let callee = match monod_result {
                Ok(monoed) => {
                    let dyn_receiver = args.iter().find_map(|arg| {
                        let operand = &arg.node;
                        let ty = operand.ty(self.caller_body, self.tcx);
                        peel_dyn_from_receiver(self.tcx, self.typing_env, ty).map(|_| (operand, ty))
                    });
                    let first_arg = dyn_receiver.map(|(operand, _)| operand);
                    let first_arg_ty = dyn_receiver.map(|(_, ty)| ty);
                    timer::measure("1.0.1.1handle_monoed_callee", || {
                        self.handle_monoed_callee(func, args, first_arg, first_arg_ty, monoed)
                    })
                }
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
        } else if let TerminatorKind::Drop { place, .. } = &terminator.kind
            && let Some(drop_impl) = self.resolve_drop_impl(place.ty(self.caller_body, self.tcx).ty)
        {
            self.callees.push(CallSite::new_with_kind(
                *self.caller_instance,
                drop_impl,
                self.constraints[&self.current_bb].constraints,
                CallKind::Drop,
            ));
        }
    }
}

impl<'tcx, 'local> SearchFunctionCall<'tcx, 'local> {
    fn new(
        tcx: ty::TyCtxt<'tcx>,
        caller_instance: &'local FunctionInstance<'tcx>,
        caller_body: &'local mir::Body<'tcx>,
        constraints: HashMap<mir::BasicBlock, BlockPath>,
        address_taken_funcs: &'local HashSet<DefId>,
    ) -> Self {
        SearchFunctionCall {
            tcx,
            caller_instance,
            caller_body,
            callees: Vec::default(),
            constraints,
            current_bb: mir::BasicBlock::from_usize(0),
            address_taken_funcs,
            typing_env: TypingEnv::post_analysis(tcx, caller_instance.def_id()),
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
        tracing::warn!("Monomorphization failed: {:?}", err);
        if let Some(fallback) = self.fallback_callable_on_mono_error(func, before_mono_ty) {
            return Some(fallback);
        }
        tracing::warn!("No callable DefId fallback available for {:?}", before_mono_ty);
        None
    }

    fn resolve_drop_impl(&self, place_ty: ty::Ty<'tcx>) -> Option<FunctionInstance<'tcx>> {
        let adt = place_ty.ty_adt_def()?;
        let destructor = adt.destructor(self.tcx)?;
        let args = match place_ty.kind() {
            ty::TyKind::Adt(_, args) => *args,
            _ => ty::GenericArgs::empty(),
        };

        match ty::Instance::try_resolve(self.tcx, self.typing_env, destructor.did, args) {
            Ok(Some(instance)) => Some(FunctionInstance::new_instance(instance)),
            _ => Some(FunctionInstance::new_non_instance(destructor.did)),
        }
    }

    /// Handle monomorphized callee
    fn handle_monoed_callee(
        &mut self,
        func: &mir::Operand<'tcx>,
        call_args: &[Spanned<mir::Operand<'tcx>>],
        first_arg: Option<&mir::Operand<'tcx>>,
        first_arg_ty: Option<ty::Ty<'tcx>>,
        monod_callee: ty::Ty<'tcx>,
    ) -> Option<FunctionInstance<'tcx>> {
        use mir::Operand::*;
        match func {
            Constant(_) => timer::measure("1.0.1.1.0handle_monod_direct_callee", || {
                self.handle_monod_direct_callee(func, call_args, first_arg, monod_callee, first_arg_ty)
            }),
            // Move or copy operands - support function pointer calls
            Move(_) | Copy(_) => timer::measure("1.0.1.1.1handle_monod_indirect_callee", || {
                self.handle_monod_indirect_callee(func, call_args, first_arg, first_arg_ty, monod_callee)
            }),
        }
    }

    /// Handle direct callee
    fn handle_monod_direct_callee(
        &mut self,
        func: &mir::Operand<'tcx>,
        call_args: &[Spanned<mir::Operand<'tcx>>],
        first_arg: Option<&mir::Operand<'tcx>>,
        monod_callee: ty::Ty<'tcx>,
        first_arg_ty: Option<ty::Ty<'tcx>>,
    ) -> Option<FunctionInstance<'tcx>> {
        debug!("Found direct call {:?}, func.ty: {:?}", func, monod_callee);
        match monod_callee.kind() {
            ty::TyKind::FnDef(..) => {
                // In this case, the callee is like a direct function call or method
                // Such as ```
                //  let a = func();
                // ```
                return timer::measure("1.0.1.1.0.0handle_monod_fn_def_callee", || {
                    self.handle_monod_fn_def_callee(call_args, first_arg, first_arg_ty, monod_callee)
                });
            }
            ty::TyKind::FnPtr(..) => timer::measure("1.0.1.1.0.1handle_monod_fn_ptr_callee", || {
                self.handle_monod_fn_ptr_callee(func, monod_callee)
            }),
            _ => tracing::warn!("skip constant (unsupported type): {:?}", monod_callee),
        }
        None
    }

    /// Handle monomorphized indirect callee
    ///
    /// When Operand is Move/Copy, the callee is a function pointer or dyn trait object
    /// We need to resolve the function pointer/trait object to get the actual callee
    fn handle_monod_indirect_callee(
        &mut self,
        func: &mir::Operand<'tcx>,
        call_args: &[Spanned<mir::Operand<'tcx>>],
        first_arg: Option<&mir::Operand<'tcx>>,
        first_arg_ty: Option<ty::Ty<'tcx>>,
        monod_callee: ty::Ty<'tcx>,
    ) -> Option<FunctionInstance<'tcx>> {
        debug!("Found indirect call {:?}, func.ty: {:?}", func, monod_callee);
        match monod_callee.kind() {
            ty::TyKind::FnDef(..) => {
                // In some cases, a local variable is assigned with a function,
                // E.g. ```
                //  let a = func;
                //  let b = a();
                // ```
                // In this case, we need to resolve the function pointer to get the actual callee.
                return timer::measure("1.0.1.1.1.0handle_monod_fn_def_callee", || {
                    self.handle_monod_fn_def_callee(call_args, first_arg, first_arg_ty, monod_callee)
                });
            }
            ty::TyKind::FnPtr(..) => timer::measure("1.0.1.1.1.1handle_monod_fn_ptr_callee", || {
                self.handle_monod_fn_ptr_callee(func, monod_callee)
            }),
            _ => {
                tracing::warn!("skip move or copy (unsupported type): {:?}", monod_callee);
            }
        }
        None
    }

    /// Handle monomorphized direct callee
    fn handle_monod_fn_def_callee(
        &mut self,
        call_args: &[Spanned<mir::Operand<'tcx>>],
        first_arg_operand: Option<&mir::Operand<'tcx>>,
        first_arg: Option<ty::Ty<'tcx>>,
        monod: ty::Ty<'tcx>,
    ) -> Option<FunctionInstance<'tcx>> {
        let ty::TyKind::FnDef(def_id, monoed_args) = monod.kind() else {
            return None;
        };

        match self.tcx.def_kind(def_id) {
            // bare function, method, associated function
            def::DefKind::Fn | def::DefKind::AssocFn => {}
            other => error!("unknown callee type: {:?}", other),
        }

        debug!("Try resolve instance: {:?}", monod);
        // use caller's context to create TypingEnv, not callee's
        let caller_def_id = self.caller_instance.def_id();
        // Use caller's typing environment for resolution
        let type_env = TypingEnv::post_analysis(self.tcx, caller_def_id);
        let result = timer::measure("fn_def resolve_instance", || {
            ty::Instance::try_resolve(self.tcx, type_env, *def_id, monoed_args)
        });

        match result {
            Err(err) => {
                error!("Instance [{:?}] resolve failed: {:?}", monod, err)
            }
            Ok(opt_instance) => {
                if let Some(instance) = opt_instance {
                    debug!("Resolved instance successfully: {:?}", instance);
                    self.maybe_handle_dyn_fn_adapter_call(call_args, first_arg_operand, first_arg, *def_id);
                    if matches!(instance.def, InstanceKind::Virtual(..)) {
                        // Virtual function call!!!!!
                        debug!("Found trait method call with dyn self: {:?}", monod);
                        timer::measure("fn_def handle_dyn_trait_method_call", || {
                            self.handle_dyn_trait_method_call(first_arg_operand, first_arg, *def_id)
                        });
                    }
                    return Some(FunctionInstance::new_instance(instance));
                } else {
                    warn!("Resolve [{:#?}] failed, trivial resolve", monod);
                    return timer::measure("fn_def trivial_resolve", || {
                        trivial_resolve(self.tcx, *def_id).or_else(|| {
                            warn!("Trivial resolve [{:?}] failed, using non-instance", def_id);
                            Some(FunctionInstance::new_non_instance(*def_id))
                        })
                    });
                }
            }
        }
        None
    }

    fn fallback_callable_on_mono_error(
        &self,
        func: &mir::Operand<'tcx>,
        before_mono_ty: ty::Ty<'tcx>,
    ) -> Option<FunctionInstance<'tcx>> {
        if let Some((def_id, _)) = operand_fn_def(func) {
            tracing::warn!(
                "Recovered DefId {:?} from operand after monomorphization failure",
                def_id
            );
            return Some(FunctionInstance::new_non_instance(def_id));
        }

        if let Some(def_id) = fallback_callable_def_id_from_ty(self.tcx, before_mono_ty) {
            tracing::warn!("Recovered DefId {:?} from type after monomorphization failure", def_id);
            return Some(FunctionInstance::new_non_instance(def_id));
        }

        None
    }

    fn maybe_handle_dyn_fn_adapter_call(
        &mut self,
        call_args: &[Spanned<mir::Operand<'tcx>>],
        first_arg_operand: Option<&mir::Operand<'tcx>>,
        first_arg: Option<ty::Ty<'tcx>>,
        def_id: DefId,
    ) {
        let requested_kind = match self.tcx.item_name(def_id).as_str() {
            "call" => Some(ty::ClosureKind::Fn),
            "call_mut" => Some(ty::ClosureKind::FnMut),
            "call_once" => Some(ty::ClosureKind::FnOnce),
            _ => None,
        };
        if let Some(kind) = requested_kind {
            for arg in call_args {
                let origin_candidates = self.origin_trace_context().resolve_dyn_fn_candidates(&arg.node, kind);
                if origin_candidates.is_empty() {
                    continue;
                }
                for cand in origin_candidates {
                    self.callees.push(CallSite::new_with_kind(
                        *self.caller_instance,
                        cand,
                        self.constraints[&self.current_bb].constraints,
                        CallKind::DynTrait,
                    ));
                }
                return;
            }
        }

        let Some((trait_id, _, _, _, _)) =
            first_arg.and_then(|arg| extract_dyn_trait_info(self.tcx, self.typing_env, arg, def_id))
        else {
            return;
        };

        let li = self.tcx.lang_items();
        let is_dyn_fn_trait = Some(trait_id) == li.fn_trait()
            || Some(trait_id) == li.fn_mut_trait()
            || Some(trait_id) == li.fn_once_trait();
        if !is_dyn_fn_trait {
            return;
        }

        timer::measure("handle_dyn_fn_adapter_call", || {
            self.handle_dyn_trait_method_call(first_arg_operand, first_arg, def_id)
        });
    }

    fn handle_monod_fn_ptr_callee(&mut self, func: &mir::Operand<'tcx>, monod: ty::Ty<'tcx>) {
        debug!("First, we try to resolve function pointer directly, func: {:?}", func);

        // First, we try to backtrace the local candidates from the function pointer operand.
        let mut local_candidates = timer::measure("resolve_fnptr_local_candidates", || {
            self.origin_trace_context().resolve_fnptr_candidates(func)
        });
        if !local_candidates.is_empty() {
            debug!("fnptr call: found {} local cands via backtrace", local_candidates.len());
            for cand in local_candidates.drain(..) {
                self.callees.push(CallSite::new_with_kind(
                    *self.caller_instance,
                    cand,
                    self.constraints[&self.current_bb].constraints,
                    CallKind::FnPtr,
                ));
            }
            return;
        }

        debug!("Failed to precision resolve function pointer, trying resolve func ptr by sig match.");
        if let ty::TyKind::FnPtr(poly_sig, _) = monod.kind() {
            let candidates = timer::measure("candidates_for_fnptr_sig", || {
                candidates_for_fnptr_sig(
                    self.tcx,
                    self.caller_instance.def_id(),
                    *poly_sig,
                    self.address_taken_funcs,
                )
            });
            if candidates.is_empty() {
                tracing::warn!("fnptr call: no cands found for sig {:?}", poly_sig);
                return;
            }
            debug!("fnptr call: found {} sig-matched cands", candidates.len());
            for cand in candidates {
                self.callees.push(CallSite::new_with_kind(
                    *self.caller_instance,
                    cand,
                    self.constraints[&self.current_bb].constraints,
                    CallKind::FnPtr,
                ));
            }
        }
    }

    /// Handle dyn trait method call
    ///
    /// def_id is the def id of the trait method call.
    /// dyn_trait_ty is the dyn trait type, which is TyKind::Dynamic.
    fn handle_dyn_trait_method_call(
        &mut self,
        first_arg_operand: Option<&mir::Operand<'tcx>>,
        first_arg: Option<ty::Ty<'tcx>>,
        def_id: DefId,
    ) -> Option<FunctionInstance<'tcx>> {
        debug!("Processing dyn trait method call: def_id={def_id:?}, first_arg={first_arg:?}");
        // For Fn/FnMut/FnOnce traits, use signature matching
        let li = self.tcx.lang_items();
        let fn_trait = li.fn_trait();
        let fn_mut_trait = li.fn_mut_trait();
        let fn_once_trait = li.fn_once_trait();

        if let Some((_tr_id, requested_kind, inputs, output)) =
            extract_dyn_fn_signature(self.tcx, self.typing_env, first_arg?)
        {
            let origin_candidates = first_arg_operand
                .map(|operand| {
                    self.origin_trace_context()
                        .resolve_dyn_fn_candidates(operand, requested_kind)
                })
                .unwrap_or_default();
            let candidates = if origin_candidates.is_empty() {
                timer::measure("candidates_for_dyn_fn_trait", || {
                    candidates_for_dyn_fn_trait(self.tcx, &inputs, output, self.address_taken_funcs)
                })
            } else {
                origin_candidates
            };
            debug!("Found {} candidates for dyn fn trait method", candidates.len());

            for cand in candidates {
                self.callees.push(CallSite::new_with_kind(
                    *self.caller_instance,
                    cand,
                    self.constraints[&self.current_bb].constraints,
                    CallKind::DynTrait,
                ));
            }
        }
        // Extract trait information
        else if let Some((tr_id, method_name, requested_kind, inputs, output)) =
            extract_dyn_trait_info(self.tcx, self.typing_env, first_arg?, def_id)
        {
            debug!("Found dyn trait method: trait={:?}, method={}", tr_id, method_name);
            let candidates = if Some(tr_id) == fn_trait || Some(tr_id) == fn_mut_trait || Some(tr_id) == fn_once_trait {
                let origin_candidates = first_arg_operand
                    .and_then(|operand| requested_kind.map(|kind| (operand, kind)))
                    .map(|(operand, kind)| self.origin_trace_context().resolve_dyn_fn_candidates(operand, kind))
                    .unwrap_or_default();
                if origin_candidates.is_empty() {
                    // if trait is Fn/FnMut/FnOnce, use signature matching
                    timer::measure("candidates_for_dyn_fn_trait", || {
                        candidates_for_dyn_fn_trait(
                            self.tcx,
                            &inputs,
                            output.unwrap_or(self.tcx.types.unit),
                            self.address_taken_funcs,
                        )
                    })
                } else {
                    origin_candidates
                }
            } else {
                // for other traits, use trait method dispatch
                timer::measure("candidates_for_dyn_normal_trait", || {
                    candidates_for_dyn_normal_trait(self.tcx, tr_id, &method_name)
                })
            };
            debug!("Found {} candidates for dyn trait method", candidates.len());

            for cand in candidates {
                self.callees.push(CallSite::new_with_kind(
                    *self.caller_instance,
                    cand,
                    self.constraints[&self.current_bb].constraints,
                    CallKind::DynTrait,
                ));
            }
        } else {
            // If cannot handle as dyn trait method call, fall back to normal resolution
            warn!("Failed to handle as dyn trait method call, falling back to normal resolution");
        }
        None
    }
    fn origin_trace_context(&self) -> OriginTraceContext<'tcx, '_> {
        OriginTraceContext::new(self.tcx, self.caller_body, self.current_bb, self.typing_env)
    }
}

// Perform monomorphization while constructing call graph
pub(crate) fn perform_mono_analysis<'tcx>(
    tcx: ty::TyCtxt<'tcx>,
    instances: Vec<FunctionInstance<'tcx>>,
    args: &crate::args::CGArgs,
) -> CallGraph<'tcx> {
    // 0. Collect all address-taken functions (RTA-like analysis)
    let address_taken_funcs = timer::measure("0.5collect_address_taken", || collect_address_taken_functions(tcx));
    timer::measure("0.6build_sig_index", || build_fn_sig_index(tcx, &address_taken_funcs));

    let mut call_graph = CallGraph::new(instances, args.without_args);
    let mut discovered = HashSet::new();

    while let Some(instance) = call_graph.instances.pop_front() {
        let _ = discovered.insert(instance);
        let call_sites = timer::measure("1.0collect_callsites", || {
            instance.collect_callsites(tcx, &address_taken_funcs)
        });

        for call_site in call_sites {
            call_graph.call_sites.push(call_site.clone());
            if discovered.contains(&call_site.callee()) {
                continue;
            }
            discovered.insert(call_site.callee());
            call_graph.instances.push_back(call_site.callee());
        }
    }

    call_graph.total_functions = discovered.len();
    tracing::info!(
        "Analysis complete: {} instances analyzed, {} call sites found",
        discovered.len(),
        call_graph.call_sites.len(),
    );

    // Deduplicate call sites if deduplication is not disabled
    if !args.no_dedup {
        tracing::info!("Deduplication enabled - removing duplicate call sites");
        timer::measure("1.1deduplicate_call_sites", || call_graph.deduplicate_call_sites());
    } else {
        tracing::info!("Deduplication disabled - keeping all call sites");
    }

    call_graph
}
