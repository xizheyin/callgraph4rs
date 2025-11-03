//! In Rust, there are two types of function calls:
//! 1. Static dispatch: the function to call is determined at compile time.
//! 2. Dynamic dispatch: the function to call is determined at runtime.

use super::{
    controlflow::{BlockPath, compute_shortest_paths},
    function::{FunctionInstance, iterate_all_functions},
    types::{CallGraph, CallSite},
};
use crate::timer;

use rustc_hir::{def, def_id::DefId};
use rustc_middle::{
    mir::{self, Terminator, TerminatorKind, visit::Visitor},
    ty::{
        self, Binder, Instance, InstanceKind, TyCtxt, TypeFoldable, TypingEnv, layout::MaybeResult,
        normalize_erasing_regions::NormalizationError,
    },
};
use std::collections::{HashMap, HashSet};
use tracing::{debug, error, info, warn};

impl<'tcx> FunctionInstance<'tcx> {
    /// the entrypoint to collect all callsites in a function instance
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

        // Compute function internal constraints,
        // which is a mapping from basic block to the path from the entry block to the basic block.
        let constraints = timer::measure("compute_constraints", || compute_shortest_paths(tcx, def_id));

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
        let caller_body = tcx.optimized_mir(caller_id);
        let mut search_callees = SearchFunctionCall::new(tcx, self, caller_body, constraints);
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
                    tracing::warn!("Callee {:?} is not monod, using non-instance", def_id);
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
        first_arg: Option<ty::Ty<'tcx>>,
        monod_ty: ty::Ty<'tcx>,
    ) -> Option<FunctionInstance<'tcx>> {
        use mir::Operand::*;
        match func {
            Constant(_) => self.handle_monod_direct_callee(func, first_arg, monod_ty),
            // Move or copy operands - 支持函数指针调用
            Move(_) | Copy(_) => self.handle_monod_indirect_callee(func, first_arg, monod_ty),
        }
    }

    /// Handle direct callee
    fn handle_monod_direct_callee(
        &mut self,
        func: &mir::Operand<'tcx>,
        first_arg: Option<ty::Ty<'tcx>>,
        monod: ty::Ty<'tcx>,
    ) -> Option<FunctionInstance<'tcx>> {
        match monod.kind() {
            ty::TyKind::FnDef(_, _) => {
                // In this case, the callee is like a direct function call or method
                // Such as ```
                //  let a = func();
                // ```
                return self.handle_monod_fn_def_callee(first_arg, monod);
            }
            ty::TyKind::FnPtr(_, _) => self.handle_monod_fn_ptr_callee(func, monod),
            _ => {
                tracing::warn!("skip constant (unsupported type): {:?}", monod);
            }
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
        first_arg: Option<ty::Ty<'tcx>>,
        monod: ty::Ty<'tcx>,
    ) -> Option<FunctionInstance<'tcx>> {
        match monod.kind() {
            ty::TyKind::FnDef(_, _) => {
                // In some cases, a local variable is assigned with a function,
                // E.g. ```
                //  let a = func;
                //  let b = a();
                // ```
                // In this case, we need to resolve the function pointer to get the actual callee.
                return self.handle_monod_fn_def_callee(first_arg, monod);
            }
            ty::TyKind::FnPtr(..) => self.handle_monod_fn_ptr_callee(func, monod),
            _ => {
                tracing::warn!("skip move or copy (unsupported type): {:?}", monod);
            }
        }
        None
    }

    /// Handle monomorphized direct callee
    fn handle_monod_fn_def_callee(
        &mut self,
        first_arg: Option<ty::Ty<'tcx>>,
        monod: ty::Ty<'tcx>,
    ) -> Option<FunctionInstance<'tcx>> {
        let ty::TyKind::FnDef(def_id, monoed_args) = monod.kind() else {
            return None;
        };
        let callee_defkind = self.tcx.def_kind(def_id);
        info!("Found direct call {:?}, kind: {:?}", monod, callee_defkind);

        match self.tcx.def_kind(def_id) {
            // bare function, method, associated function
            def::DefKind::Fn | def::DefKind::AssocFn => {
                debug!("Try resolve instance: {:?}", monod);
                // use caller's context to create TypingEnv, not callee's
                let caller_def_id = self.caller_instance.def_id();
                // Use caller's typing environment for resolution
                let type_env = TypingEnv::post_analysis(self.tcx, caller_def_id);
                let result = ty::Instance::try_resolve(self.tcx, type_env, *def_id, monoed_args);

                match result {
                    Err(err) => {
                        error!("Instance [{:?}] resolve failed: {:?}", monod, err)
                    }
                    Ok(opt_instance) => {
                        if let Some(instance) = opt_instance {
                            info!("Resolved instance successfully: {:?}", instance);
                            if matches!(instance.def, InstanceKind::Virtual(..)) {
                                // Virtual function call!!!!!
                                info!("Found trait method call with dyn self: {:?}", monod);
                                self.handle_dyn_trait_method_call(first_arg, *def_id);
                            }
                            return Some(FunctionInstance::new_instance(instance));
                        } else {
                            warn!("Resolve [{:#?}] failed, trivial resolve", monod);
                            return trivial_resolve(self.tcx, *def_id).or_else(|| {
                                warn!("Trivial resolve [{:?}] failed, using non-instance", def_id);
                                Some(FunctionInstance::new_non_instance(*def_id))
                            });
                        }
                    }
                }
            }
            other => error!("unknown callee type: {:?}", other),
        }

        None
    }

    fn handle_monod_fn_ptr_callee(&mut self, func: &mir::Operand<'tcx>, monod: ty::Ty<'tcx>) {
        tracing::info!("First, we try to resolve function pointer directly, func: {:?}", func);

        // 先尝试基于当前基本块的本地回溯解析 fn 指针变量来源，提取明确的 FnDef 候选
        let mut local_candidates = self.resolve_fnptr_local_candidates(func);
        if !local_candidates.is_empty() {
            tracing::info!(
                "fnptr call: found {} local candidates via backtrace",
                local_candidates.len()
            );
            for cand in local_candidates.drain(..) {
                self.callees.push(CallSite::new(
                    *self.caller_instance,
                    cand,
                    self.constraints[&self.current_bb].constraints,
                ));
            }
            return;
        }

        tracing::info!("Failed,next, we try to resolve function pointer by signature matching.");
        if let ty::TyKind::FnPtr(poly_sig, _) = monod.kind() {
            let candidates = candidates_for_fnptr_sig(self.tcx, *poly_sig);
            if candidates.is_empty() {
                tracing::warn!("fnptr call: no cands found for sig {:?}", poly_sig);
            } else {
                tracing::info!("fnptr call: found {} sig-matched cands", candidates.len());
                for cand in candidates {
                    self.callees.push(CallSite::new(
                        *self.caller_instance,
                        cand,
                        self.constraints[&self.current_bb].constraints,
                    ));
                }
            }
        }
    }

    /// Handle dyn trait method call
    ///
    /// def_id is the def id of the trait method call.
    /// dyn_trait_ty is the dyn trait type, which is TyKind::Dynamic.
    fn handle_dyn_trait_method_call(
        &mut self,
        first_arg: Option<ty::Ty<'tcx>>,
        def_id: DefId,
    ) -> Option<FunctionInstance<'tcx>> {
        info!("Processing dyn trait method call: def_id={def_id:?}, first_arg={first_arg:?}");
        // For Fn/FnMut/FnOnce traits, use signature matching
        let li = self.tcx.lang_items();
        let fn_trait = li.fn_trait();
        let fn_mut_trait = li.fn_mut_trait();
        let fn_once_trait = li.fn_once_trait();

        // 提取 trait 信息
        if let Some((tr_id, method_name, inputs, output)) = self.extract_dyn_trait_info(first_arg, def_id) {
            info!("Found dyn trait method: trait={:?}, method={}", tr_id, method_name);
            let candidates = if Some(tr_id) == fn_trait || Some(tr_id) == fn_mut_trait || Some(tr_id) == fn_once_trait {
                // if trait is Fn/FnMut/FnOnce, use signature matching
                candidates_for_dyn_fn_trait(self.tcx, &inputs, output)
            } else {
                // for other traits, use trait method dispatch
                candidates_for_dyn_normal_trait(self.tcx, tr_id, &method_name)
            };
            info!("Found {} candidates for dyn trait method", candidates.len());

            for cand in candidates {
                self.callees.push(CallSite::new(
                    *self.caller_instance,
                    cand,
                    self.constraints[&self.current_bb].constraints,
                ));
            }
        } else {
            // If cannot handle as dyn trait method call, fall back to normal resolution
            warn!("Failed to handle as dyn trait method call, falling back to normal resolution");
        }
        None
    }
    // extract trait information from dyn Trait type
    // Note:
    // The last two return value are the args tuple and output type of the method.
    // It is valid only when the method is a Fn/FnMut/FnOnce trait method.
    fn extract_dyn_trait_info(
        &self,
        first_arg: Option<ty::Ty<'tcx>>,
        method_def_id: DefId,
    ) -> Option<(DefId, String, Vec<ty::Ty<'tcx>>, ty::Ty<'tcx>)> {
        let arg0 = first_arg?;
        let typing_env = TypingEnv::post_analysis(self.tcx, self.caller_instance.def_id());
        let preds = if let Some(ty) = peel_dyn_from_receiver(self.tcx, typing_env, arg0)
            && let ty::TyKind::Dynamic(preds, _, _) = ty.kind()
        {
            preds
        } else {
            return None;
        };

        let li = self.tcx.lang_items();
        let fn_trait = li.fn_trait();
        let fn_mut_trait = li.fn_mut_trait();
        let fn_once_trait = li.fn_once_trait();
        let output_assoc = li.fn_once_output();

        let mut trait_def_id: Option<DefId> = None;
        let mut method_name: Option<String> = None;
        let mut maybe_args_tuple: Option<ty::Ty<'tcx>> = None;
        let mut maybe_output: Option<ty::Ty<'tcx>> = None;

        for p in preds.iter() {
            match p.skip_binder() {
                ty::ExistentialPredicate::Trait(tr) => {
                    // 检查是否是 Fn/FnMut/FnOnce trait
                    if let Some(fn_id) = fn_trait {
                        if tr.def_id == fn_id && tr.args.len() == 1 {
                            trait_def_id = Some(fn_id);
                            method_name = Some("call".to_string());
                            let args_tuple = tr.args.type_at(0);
                            maybe_args_tuple = Some(args_tuple);
                        }
                    }
                    if let Some(fn_mut_id) = fn_mut_trait {
                        if tr.def_id == fn_mut_id && tr.args.len() == 1 {
                            trait_def_id = Some(fn_mut_id);
                            method_name = Some("call_mut".to_string());
                            let args_tuple = tr.args.type_at(0);
                            maybe_args_tuple = Some(args_tuple);
                        }
                    }
                    if let Some(fn_once_id) = fn_once_trait {
                        if tr.def_id == fn_once_id && tr.args.len() == 1 {
                            trait_def_id = Some(fn_once_id);
                            method_name = Some("call_once".to_string());
                            let args_tuple = tr.args.type_at(0);
                            maybe_args_tuple = Some(args_tuple);
                        }
                    }

                    // 如果不是 Fn trait，则是普通的 trait
                    // 对于普通 trait，我们可以从 method_def_id 获取真正的方法名
                    if trait_def_id.is_none() {
                        trait_def_id = Some(tr.def_id);
                        // 从 method_def_id 获取方法名
                        let method_name_str = self.tcx.item_name(method_def_id).to_string();
                        method_name = Some(method_name_str);
                    }
                }
                ty::ExistentialPredicate::Projection(pr) => {
                    if let Some(out_id) = output_assoc {
                        if pr.def_id == out_id {
                            maybe_output = Some(pr.term.expect_type());
                        }
                    }
                }
                ty::ExistentialPredicate::AutoTrait(_) => {}
            }
        }
        println!(
            "Extracted dyn trait info: trait_def_id={:?}, method_name={:?}, args_tuple={:?}, output={:?}",
            trait_def_id, method_name, maybe_args_tuple, maybe_output
        );
        let trait_id = trait_def_id?;
        let method = method_name?;

        // 对于 Fn traits，我们有完整的签名信息
        if let (Some(args_tuple), Some(output)) = (maybe_args_tuple, maybe_output) {
            let inputs = match args_tuple.kind() {
                ty::TyKind::Tuple(elems) => elems.iter().collect(),
                _ => return None,
            };
            return Some((trait_id, method, inputs, output));
        }

        // 对于普通 trait，返回基本信息，签名需要从其他地方获取
        Some((trait_id, method, vec![], self.tcx.types.unit))
    }
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
            tracing::debug!(
                "Found Call => callee: {:?}, func.ty: {:?}",
                func,
                func.ty(self.caller_body, self.tcx)
            );

            let typing_env = TypingEnv::post_analysis(self.tcx, self.caller_instance.def_id());

            let before_mono_ty = func.ty(self.caller_body, self.tcx);

            // Perform monomorphization
            let monod_result = monomorphize(
                self.tcx,
                typing_env,
                self.caller_instance.instance().expect("instance is None"),
                before_mono_ty,
            );

            let callee = match monod_result {
                Ok(monoed) => {
                    //calculate first argument type for potential dyn trait method call
                    let first_arg_ty = args.iter().next().map(|arg| arg.node.ty(self.caller_body, self.tcx));
                    self.handle_monoed_callee(func, first_arg_ty, monoed)
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
        }
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
        let instance = ty::Instance::try_resolve(tcx, TypingEnv::post_analysis(tcx, def_id), *def_id, args);
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
        timer::measure("deduplicate_call_sites", || call_graph.deduplicate_call_sites());
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
    instance.try_instantiate_mir_and_normalize_erasing_regions(tcx, typing_env, ty::EarlyBinder::bind(value))
}

/// Find function candidates that match the given function pointer signature
/// FIXME: needs further optimization
pub(crate) fn candidates_for_fnptr_sig<'tcx>(
    tcx: TyCtxt<'tcx>,
    sig: Binder<'tcx, ty::FnSigTys<TyCtxt<'tcx>>>,
) -> Vec<FunctionInstance<'tcx>> {
    let sig = tcx.normalize_erasing_late_bound_regions(TypingEnv::fully_monomorphized(), sig);
    let sig_inputs = sig.inputs();
    let sig_output = sig.output();

    let candidates = iterate_all_functions(
        tcx,
        |def_id| {
            let type_env = ty::TypingEnv::post_analysis(tcx, def_id);

            let candidate_sig = tcx.normalize_erasing_late_bound_regions(type_env, tcx.fn_sig(def_id).skip_binder());

            // check if the candidate function has the same number of inputs as the signature
            if candidate_sig.inputs().len() != sig_inputs.len() {
                return false;
            }

            // check if the candidate function has the same return type as the signature
            let return_types_match = candidate_sig.output() == sig_output;
            // check if the candidate function has the same input types as the signature
            let inputs_match = candidate_sig
                .inputs()
                .iter()
                .zip(sig_inputs.iter())
                .all(|(cand_input, sig_input)| *cand_input == *sig_input);

            return_types_match && inputs_match
        },
        |def_id| {
            if let Some(instance) = trivial_resolve(tcx, def_id) {
                Some(instance)
            } else {
                Some(FunctionInstance::new_non_instance(def_id))
            }
        },
    );

    tracing::debug!(
        "Found {} candidates for fnptr signature with {} inputs",
        candidates.len(),
        sig_inputs.len()
    );
    candidates
}

// FIXME: needs further optimization
fn candidates_for_dyn_fn_trait<'tcx>(
    tcx: TyCtxt<'tcx>,
    inputs: &[ty::Ty<'tcx>],
    output: ty::Ty<'tcx>,
) -> Vec<FunctionInstance<'tcx>> {
    fn types_match_ignoring_regions<'tcx>(tcx: TyCtxt<'tcx>, a: ty::Ty<'tcx>, b: ty::Ty<'tcx>) -> bool {
        let a_erased = tcx.erase_regions(a);
        let b_erased = tcx.erase_regions(b);
        println!(
            "a= {:?} b= {:?} a_erased= {:?} b_erased= {:?}",
            a, b, a_erased, b_erased
        );
        a_erased == b_erased
    }

    let mut candidates = Vec::new();

    for owner_def_id in tcx.hir_body_owners() {
        let ty = tcx.type_of(owner_def_id).skip_binder();
        if let ty::TyKind::FnDef(fn_def_id, _args) = ty.kind() {
            let candidate_sig = tcx.fn_sig(*fn_def_id).skip_binder();

            println!(
                "candidate_sig= {:?} candidate_sig.inputs().skip_binder().len()= {:?}",
                candidate_sig,
                candidate_sig.inputs().skip_binder().len()
            );
            if candidate_sig.inputs().skip_binder().len() == inputs.len() {
                let return_types_match =
                    types_match_ignoring_regions(tcx, candidate_sig.output().skip_binder(), output);

                let inputs_match = candidate_sig
                    .inputs()
                    .skip_binder()
                    .iter()
                    .zip(inputs.iter())
                    .all(|(cand_input, want_input)| types_match_ignoring_regions(tcx, *cand_input, *want_input));

                if return_types_match && inputs_match {
                    if let Some(instance) = trivial_resolve(tcx, *fn_def_id) {
                        candidates.push(instance);
                    } else {
                        candidates.push(FunctionInstance::new_non_instance(*fn_def_id));
                    }
                }
            }
        }
    }

    tracing::debug!(
        "Found {} candidates for dyn fn signature with {} inputs",
        candidates.len(),
        inputs.len()
    );
    candidates
}

// 为 dyn trait 查找候选函数
fn candidates_for_dyn_normal_trait<'tcx>(
    tcx: TyCtxt<'tcx>,
    tr_id: DefId,
    method_name: &str,
) -> Vec<FunctionInstance<'tcx>> {
    // For ordinary traits, find the methods of all types that implement the trait
    // and supplement the trait's own default implementation (if any) to ensure no omission
    let mut candidates = Vec::new();

    // Find the default method of the trait (if any)
    let trait_method_def_id = tcx.associated_item_def_ids(tr_id).iter().find_map(|&item_def_id| {
        let item = tcx.associated_item(item_def_id);
        if item.name().to_string() == method_name && matches!(item.kind, ty::AssocKind::Fn { .. }) {
            Some(item_def_id)
        } else {
            None
        }
    });

    // Traverse all types that implement the trait
    let all_impls = tcx.all_impls(tr_id);
    for impl_def_id in all_impls {
        // Find the specified method in the impl
        let mut found_override = false;
        for &item_def_id in tcx.associated_item_def_ids(impl_def_id) {
            let item = tcx.associated_item(item_def_id);

            // Check if it's the method we're looking for (impl override)
            if item.name().to_string() == method_name && matches!(item.kind, ty::AssocKind::Fn { .. }) {
                found_override = true;
                if let Some(instance) = trivial_resolve(tcx, item_def_id) {
                    candidates.push(instance);
                } else {
                    candidates.push(FunctionInstance::new_non_instance(item_def_id));
                }
            }
        }

        // 如果该 impl 没有重载该方法，且 trait 有同名方法，则加入 trait 的默认实现
        if !found_override {
            if let Some(def_id) = trait_method_def_id {
                if let Some(instance) = trivial_resolve(tcx, def_id) {
                    candidates.push(instance);
                } else {
                    candidates.push(FunctionInstance::new_non_instance(def_id));
                }
            }
        }
    }

    tracing::debug!(
        "Found {} candidates for dyn trait {} method {}",
        candidates.len(),
        tcx.def_path_str(tr_id),
        method_name
    );
    candidates
}

/// 从类型中剥去所有 dyn 包裹，返回最内层的类型
fn peel_dyn_from_receiver<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    ty: ty::Ty<'tcx>,
) -> Option<ty::Ty<'tcx>> {
    match ty.kind() {
        ty::TyKind::Dynamic(..) => Some(ty),

        // &T / &mut T
        ty::TyKind::Ref(_, inner, _) => peel_dyn_from_receiver(tcx, typing_env, *inner),

        // *const T / *mut T
        ty::TyKind::RawPtr(inner, _) => peel_dyn_from_receiver(tcx, typing_env, *inner),

        // Box<T> 特例
        _ if ty.is_box_global(tcx) => {
            let inner = ty.expect_boxed_ty();
            peel_dyn_from_receiver(tcx, typing_env, inner)
        }

        // 透明 newtype 或通用 ADT 包裹（Pin<T>、Rc<T>、Arc<T>、Cow<'_, T> 等）
        ty::TyKind::Adt(adt_def, args) => {
            // 先尝试遍历泛型参数中可能的类型参数
            for i in 0..args.len() {
                if let Some(inner) = args[i].as_type() {
                    if let Some(d) = peel_dyn_from_receiver(tcx, typing_env, inner) {
                        return Some(d);
                    }
                }
            }
            // 透明 newtype：沿字段类型继续剥
            if adt_def.repr().transparent() && !adt_def.is_union() {
                let variant = adt_def.non_enum_variant();
                for field in &variant.fields {
                    let fty = field.ty(tcx, args);
                    if let Some(d) = peel_dyn_from_receiver(tcx, typing_env, fty) {
                        return Some(d);
                    }
                }
            }
            None
        }

        // 兜底：尝试不定长尾部（某些包装或派生类型能直接剥出 dyn）
        _ => {
            let tail = tcx.struct_tail_for_codegen(ty, typing_env);
            if matches!(tail.kind(), ty::TyKind::Dynamic(..)) {
                Some(tail)
            } else {
                None
            }
        }
    }
}

impl<'tcx, 'local> SearchFunctionCall<'tcx, 'local> {
    fn resolve_fnptr_local_candidates(&mut self, func: &mir::Operand<'tcx>) -> Vec<FunctionInstance<'tcx>> {
        use mir::Operand::{Copy as OpCopy, Move as OpMove};
        use rustc_middle::mir::{Rvalue, StatementKind};

        let mut out = Vec::new();
        let target_local = match func {
            OpMove(place) | OpCopy(place) => place.local,
            _ => return out,
        };

        // 在当前基本块逆序查找对该 local 的最近赋值
        let bb = &self.caller_body.basic_blocks[self.current_bb];
        let mut follow_local = target_local;

        for stmt in bb.statements.iter().rev() {
            if let StatementKind::Assign(assign) = &stmt.kind {
                let (lhs, rhs) = &**assign;
                if lhs.local != follow_local {
                    continue;
                }
                match rhs {
                    Rvalue::Use(op) => {
                        if let Some((def_id, args)) = operand_const_fn_def(op) {
                            // 解析成具体实例
                            let caller_def_id = self.caller_instance.def_id();
                            let type_env = TypingEnv::post_analysis(self.tcx, caller_def_id);
                            match ty::Instance::try_resolve(self.tcx, type_env, def_id, args) {
                                Ok(Some(inst)) => out.push(FunctionInstance::new_instance(inst)),
                                _ => {
                                    if let Some(inst) = trivial_resolve(self.tcx, def_id) {
                                        out.push(inst);
                                    } else {
                                        out.push(FunctionInstance::new_non_instance(def_id));
                                    }
                                }
                            }
                            break;
                        }
                        // 链式复制：继续跟踪新的来源局部
                        match op {
                            OpMove(p2) | OpCopy(p2) => {
                                follow_local = p2.local;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    Rvalue::Cast(_, op, _ty) => {
                        if let Some((def_id, args)) = operand_const_fn_def(op) {
                            let caller_def_id = self.caller_instance.def_id();
                            let type_env = TypingEnv::post_analysis(self.tcx, caller_def_id);
                            match ty::Instance::try_resolve(self.tcx, type_env, def_id, args) {
                                Ok(Some(inst)) => out.push(FunctionInstance::new_instance(inst)),
                                _ => {
                                    if let Some(inst) = trivial_resolve(self.tcx, def_id) {
                                        out.push(inst);
                                    } else {
                                        out.push(FunctionInstance::new_non_instance(def_id));
                                    }
                                }
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }

        out
    }
}
/// 逆向回溯当前基本块中对 fn 指针局部的赋值来源，解析出明确的函数项候选
fn operand_const_fn_def<'tcx>(op: &mir::Operand<'tcx>) -> Option<(DefId, ty::GenericArgsRef<'tcx>)> {
    op.const_fn_def()
}
