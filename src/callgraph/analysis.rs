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
            /// When Operand is Move/Copy, the callee is a function pointer or dyn trait object
            /// We need to resolve the function pointer/trait object to get the actual callee
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
                    }
                    // 新增：支持 dyn trait 对象的动态分派
                    ty::TyKind::Dynamic(_, _, _) => {
                        if let Some((trait_id, method_name, inputs, output)) =
                            extract_dyn_trait_info(self.tcx, monod_ty)
                        {
                            let candidates = candidates_for_dyn_trait(
                                self.tcx,
                                trait_id,
                                &method_name,
                                &inputs,
                                output,
                            );
                            if candidates.is_empty() {
                                tracing::warn!(
                                    "dyn call: no candidates found for trait {} method {} with inputs={:?}, output={:?}",
                                    self.tcx.def_path_str(trait_id), method_name, inputs, output
                                );
                            } else {
                                tracing::info!(
                                    "dyn call: found {} candidates for trait {} method {}",
                                    candidates.len(),
                                    self.tcx.def_path_str(trait_id),
                                    method_name
                                );
                                for cand in candidates {
                                    self.callees.push(CallSite::new(
                                        *self.caller_instance,
                                        cand,
                                        self.constraints[&self.current_bb].constraints,
                                    ));
                                }
                            }
                        } else {
                            tracing::warn!(
                                "dyn call: failed to extract trait info: {:?}",
                                monod_ty
                            );
                        }
                    }
                    // Support &dyn T / *const dyn T / Box<dyn T> / Rc<dyn T> / Arc<dyn T>
                    ty::TyKind::Ref(_, inner, _) | ty::TyKind::RawPtr(inner, ..) => {
                        // This is for &dyn T / *const dyn T
                        return self.handle_monoed_indirect_callee(*inner);
                    }
                    ty::TyKind::Adt(adt, args) => {
                        // This is for Box<dyn T> / Rc<dyn T> / Arc<dyn T>
                        if is_box_like(self.tcx, adt.did()) {
                            let inner = args.type_at(0);
                            return self.handle_monoed_indirect_callee(inner);
                        }
                        tracing::warn!("skip move or copy (unsupported ADT type): {:?}", monod_ty);
                    }
                    _ => {
                        tracing::warn!("skip move or copy (unsupported type): {:?}", monod_ty);
                    }
                }
                None
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

                let inputs_match = candidate_sig
                    .inputs()
                    .skip_binder()
                    .iter()
                    .zip(sig_inputs.iter())
                    .all(|(cand_input, sig_input)| {
                        types_match_ignoring_regions(tcx, *cand_input, *sig_input)
                    });

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
        "Found {} candidates for fnptr signature with {} inputs",
        candidates.len(),
        sig_inputs.len()
    );
    candidates
}

// 辅助：判断常见 box-like 智能指针 (Box / Rc / Arc)
fn is_box_like(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    let name = tcx.def_path_str(def_id);
    name.ends_with("Box") || name.ends_with("Rc") || name.ends_with("Arc")
}

// 辅助：从 dyn Trait 类型中提取完整的 trait 信息
fn extract_dyn_trait_info<'tcx>(
    tcx: TyCtxt<'tcx>,
    dyn_ty: ty::Ty<'tcx>,
) -> Option<(DefId, String, Vec<ty::Ty<'tcx>>, ty::Ty<'tcx>)> {
    let (preds, _reg, _repr) = match dyn_ty.kind() {
        ty::TyKind::Dynamic(preds, reg, repr) => (preds, reg, repr),
        _ => return None,
    };

    let li = tcx.lang_items();
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
                    if tr.def_id == fn_id {
                        trait_def_id = Some(fn_id);
                        method_name = Some("call".to_string());
                        let args_tuple = tr.args.type_at(1);
                        maybe_args_tuple = Some(args_tuple);
                    }
                }
                if let Some(fn_mut_id) = fn_mut_trait {
                    if tr.def_id == fn_mut_id {
                        trait_def_id = Some(fn_mut_id);
                        method_name = Some("call_mut".to_string());
                        let args_tuple = tr.args.type_at(1);
                        maybe_args_tuple = Some(args_tuple);
                    }
                }
                if let Some(fn_once_id) = fn_once_trait {
                    if tr.def_id == fn_once_id {
                        trait_def_id = Some(fn_once_id);
                        method_name = Some("call_once".to_string());
                        let args_tuple = tr.args.type_at(1);
                        maybe_args_tuple = Some(args_tuple);
                    }
                }

                // 如果不是 Fn trait，则是普通的 trait
                // 对于普通 trait，我们只能获取 trait_def_id，方法名需要从调用上下文推断
                if trait_def_id.is_none() {
                    trait_def_id = Some(tr.def_id);
                    // 对于普通 trait，我们暂时无法确定具体的方法名
                    // 这需要从 MIR 调用指令中获取更多信息
                    method_name = Some("unknown_method".to_string());
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
    Some((trait_id, method, vec![], tcx.types.unit))
}

// 为 dyn trait 查找候选函数
fn candidates_for_dyn_trait<'tcx>(
    tcx: TyCtxt<'tcx>,
    trait_id: DefId,
    method_name: &str,
    inputs: &[ty::Ty<'tcx>],
    output: ty::Ty<'tcx>,
) -> Vec<FunctionInstance<'tcx>> {
    // 对于 Fn/FnMut/FnOnce traits，使用签名匹配
    let li = tcx.lang_items();
    let fn_trait = li.fn_trait();
    let fn_mut_trait = li.fn_mut_trait();
    let fn_once_trait = li.fn_once_trait();
    
    if Some(trait_id) == fn_trait || Some(trait_id) == fn_mut_trait || Some(trait_id) == fn_once_trait {
        return candidates_for_dyn_fn_sig(tcx, inputs, output);
    }
    
    // 对于普通 trait，暂时使用简化的实现
    // 这里我们遍历所有函数，检查它们是否实现了该 trait
    let mut candidates = Vec::new();
    
    // 简化版本：遍历所有函数，暂时不做 trait 实现检查
    // 在实际使用中，这会产生一些误报，但至少能工作
    for owner_def_id in tcx.hir_body_owners() {
        let ty = tcx.type_of(owner_def_id).skip_binder();
        if let ty::TyKind::FnDef(fn_def_id, _args) = ty.kind() {
            // 简单的名称匹配（这是一个简化的实现）
            let fn_name = tcx.def_path_str(*fn_def_id);
            if method_name == "unknown_method" || fn_name.contains(method_name) {
                if let Some(instance) = trivial_resolve(tcx, *fn_def_id) {
                    candidates.push(instance);
                } else {
                    candidates.push(FunctionInstance::new_non_instance(*fn_def_id));
                }
            }
        }
    }
    
    tracing::debug!(
        "Found {} candidates for dyn trait {} method {} (simplified implementation)",
        candidates.len(),
        tcx.def_path_str(trait_id),
        method_name
    );
    candidates
}

// 基于 (inputs, output) 收集候选函数
fn candidates_for_dyn_fn_sig<'tcx>(
    tcx: TyCtxt<'tcx>,
    inputs: &[ty::Ty<'tcx>],
    output: ty::Ty<'tcx>,
) -> Vec<FunctionInstance<'tcx>> {
    fn types_match_ignoring_regions<'tcx>(
        tcx: TyCtxt<'tcx>,
        a: ty::Ty<'tcx>,
        b: ty::Ty<'tcx>,
    ) -> bool {
        let a_erased = tcx.erase_regions(a);
        let b_erased = tcx.erase_regions(b);
        a_erased == b_erased
    }

    let mut candidates = Vec::new();

    for owner_def_id in tcx.hir_body_owners() {
        let ty = tcx.type_of(owner_def_id).skip_binder();
        if let ty::TyKind::FnDef(fn_def_id, _args) = ty.kind() {
            let candidate_sig = tcx.fn_sig(*fn_def_id).skip_binder();

            if candidate_sig.inputs().skip_binder().len() == inputs.len() {
                let return_types_match =
                    types_match_ignoring_regions(tcx, candidate_sig.output().skip_binder(), output);

                let inputs_match = candidate_sig
                    .inputs()
                    .skip_binder()
                    .iter()
                    .zip(inputs.iter())
                    .all(|(cand_input, want_input)| {
                        types_match_ignoring_regions(tcx, *cand_input, *want_input)
                    });

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
