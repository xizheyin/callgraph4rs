use rustc_hir::{def, def_id::DefId};
use rustc_middle::ty::{Instance, TyCtxt, TypeFoldable, TypingEnv};
use rustc_middle::{
    mir::{self, Terminator, TerminatorKind},
    ty::{self},
};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::constraint_utils::{self, BlockPath};

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

    /// Deduplicate call sites, keeping only the one with the minimum constraint count
    /// for each unique caller-callee pair
    pub fn deduplicate_call_sites(&mut self) {
        // Create a map to track the call site with minimum constraint_cnt for each caller-callee pair
        let mut min_constraints: HashMap<(FunctionInstance<'tcx>, FunctionInstance<'tcx>), usize> =
            HashMap::new();
        let mut min_indices: HashMap<(FunctionInstance<'tcx>, FunctionInstance<'tcx>), usize> =
            HashMap::new();

        // Find minimum constraint count for each caller-callee pair
        for (index, call_site) in self.call_sites.iter().enumerate() {
            let key = (call_site._caller, call_site.callee);

            if let Some(existing_cnt) = min_constraints.get(&key) {
                if call_site.constraint_cnt < *existing_cnt {
                    min_constraints.insert(key, call_site.constraint_cnt);
                    min_indices.insert(key, index);
                }
            } else {
                min_constraints.insert(key, call_site.constraint_cnt);
                min_indices.insert(key, index);
            }
        }

        // Keep only the call sites with minimum constraint counts
        let indices_to_keep: HashSet<usize> = min_indices.values().cloned().collect();

        // Create a new call_sites vector with only the deduplicated entries
        let mut deduplicated_call_sites = Vec::new();

        for (index, call_site) in self.call_sites.iter().enumerate() {
            if indices_to_keep.contains(&index) {
                deduplicated_call_sites.push(call_site.clone());
            }
        }

        tracing::info!(
            "Deduplicated call sites: {} -> {} entries",
            self.call_sites.len(),
            deduplicated_call_sites.len()
        );

        self.call_sites = deduplicated_call_sites;
    }

    /// 将调用图格式化为可读的文本
    pub fn format_call_graph(&self, tcx: TyCtxt<'tcx>) -> String {
        let mut result = String::new();

        result.push_str("Call Graph:\n");
        result.push_str("===========\n\n");

        // 按调用者组织调用
        let mut calls_by_caller: HashMap<FunctionInstance<'tcx>, Vec<&CallSite<'tcx>>> =
            HashMap::new();

        for call_site in &self.call_sites {
            calls_by_caller
                .entry(call_site._caller)
                .or_default()
                .push(call_site);
        }

        // 对调用者排序以获得一致的输出
        let mut callers: Vec<FunctionInstance<'tcx>> = calls_by_caller.keys().cloned().collect();
        callers.sort_by_key(|caller| format!("{:?}", caller));

        for caller in callers {
            // 获取调用者名称
            let caller_name = self.function_instance_to_string(tcx, caller);
            result.push_str(&format!("Function: {}\n", caller_name));

            // 获取此调用者的所有调用
            if let Some(calls) = calls_by_caller.get(&caller) {
                // 按被调用者和约束计数排序
                let mut sorted_calls = calls.clone();
                sorted_calls.sort_by(|a, b| {
                    let a_name = self.function_instance_to_string(tcx, a.callee);
                    let b_name = self.function_instance_to_string(tcx, b.callee);
                    a_name
                        .cmp(&b_name)
                        .then_with(|| a.constraint_cnt.cmp(&b.constraint_cnt))
                });

                // 输出调用信息
                for call in sorted_calls {
                    let callee_name = self.function_instance_to_string(tcx, call.callee);
                    result.push_str(&format!(
                        "  -> {} [constraint: {}]\n",
                        callee_name, call.constraint_cnt
                    ));
                }

                result.push_str("\n");
            }
        }

        result
    }

    /// 将函数实例转换为可读字符串
    fn function_instance_to_string(
        &self,
        tcx: TyCtxt<'tcx>,
        instance: FunctionInstance<'tcx>,
    ) -> String {
        match instance {
            FunctionInstance::Instance(inst) => {
                let def_id = inst.def_id();
                // 获取可读的函数名称
                let def_path = tcx.def_path_str(def_id);

                // 添加泛型参数信息，如果有的话
                if inst.args.len() > 0 {
                    format!("{}<{:?}>", def_path, inst.args)
                } else {
                    def_path
                }
            }
            FunctionInstance::NonInstance(def_id) => {
                // 对于非实例，只显示路径
                format!("{} (non-instance)", tcx.def_path_str(def_id))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallSite<'tcx> {
    _caller: FunctionInstance<'tcx>,
    callee: FunctionInstance<'tcx>,
    constraint_cnt: usize,
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
            Self::Instance(instance) => Some(*instance),
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
        let constraints = get_constraints(tcx, def_id);
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
                            _caller: *self.caller_instance,
                            callee,
                            constraint_cnt: self.constraints[&self.current_bb].constraints,
                        });
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

fn trivial_resolve(tcx: ty::TyCtxt<'_>, def_id: DefId) -> Option<FunctionInstance<'_>> {
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

pub fn perform_mono_analysis<'tcx>(
    tcx: ty::TyCtxt<'tcx>,
    instances: Vec<FunctionInstance<'tcx>>,
    options: &crate::args::CGArgs,
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
            call_graph.instances.push_back(call_site.callee);
            call_graph.call_sites.push(call_site);
        }
    }

    // Deduplicate call sites if deduplication is not disabled
    if !options.no_dedup {
        tracing::info!("Deduplication enabled - removing duplicate call sites");
        call_graph.deduplicate_call_sites();
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

fn get_constraints(tcx: ty::TyCtxt, def_id: DefId) -> HashMap<mir::BasicBlock, BlockPath> {
    let mir = tcx.optimized_mir(def_id);
    constraint_utils::compute_shortest_paths(mir)
}
