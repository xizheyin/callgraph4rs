use super::{
    function::FunctionInstance,
    resolution::helpers::trivial_resolve,
    summary::{ReturnSummary, summarize_callable_return},
};
use rustc_abi::FieldIdx;
use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{self, ProjectionElem},
    ty::{self, Instance, TyCtxt, TypingEnv},
};
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CallableOrigin<'tcx> {
    Function(FunctionInstance<'tcx>),
    Closure {
        def_id: DefId,
        args: ty::GenericArgsRef<'tcx>,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct OriginTraceContext<'tcx, 'a> {
    tcx: TyCtxt<'tcx>,
    caller_body: &'a mir::Body<'tcx>,
    current_bb: mir::BasicBlock,
    typing_env: TypingEnv<'tcx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TrackedPlace {
    local: mir::Local,
    field: Option<FieldIdx>,
}

impl TrackedPlace {
    fn from_place(place: mir::Place<'_>) -> Option<Self> {
        match place.as_ref().projection {
            [] => Some(Self {
                local: place.local,
                field: None,
            }),
            [ProjectionElem::Field(field, _)] => Some(Self {
                local: place.local,
                field: Some(*field),
            }),
            _ => None,
        }
    }

    fn matches_exact_lhs(self, lhs: mir::Place<'_>) -> bool {
        match (self.field, lhs.as_ref().projection) {
            (None, []) => lhs.local == self.local,
            (Some(field), [ProjectionElem::Field(lhs_field, _)]) => lhs.local == self.local && *lhs_field == field,
            _ => false,
        }
    }

    fn matches_whole_local(self, lhs: mir::Place<'_>) -> bool {
        lhs.local == self.local && lhs.as_ref().projection.is_empty()
    }
}

impl<'tcx, 'a> OriginTraceContext<'tcx, 'a> {
    pub(crate) fn new(
        tcx: TyCtxt<'tcx>,
        caller_body: &'a mir::Body<'tcx>,
        current_bb: mir::BasicBlock,
        typing_env: TypingEnv<'tcx>,
    ) -> Self {
        Self {
            tcx,
            caller_body,
            current_bb,
            typing_env,
        }
    }

    pub(crate) fn resolve_fnptr_candidates(&self, operand: &mir::Operand<'tcx>) -> Vec<FunctionInstance<'tcx>> {
        self.lower_origins_to_candidates(self.trace_origins(operand), None)
    }

    pub(crate) fn resolve_dyn_fn_candidates(
        &self,
        receiver: &mir::Operand<'tcx>,
        requested_kind: ty::ClosureKind,
    ) -> Vec<FunctionInstance<'tcx>> {
        self.lower_origins_to_candidates(self.trace_origins(receiver), Some(requested_kind))
    }

    fn lower_origins_to_candidates(
        &self,
        origins: Vec<CallableOrigin<'tcx>>,
        requested_kind: Option<ty::ClosureKind>,
    ) -> Vec<FunctionInstance<'tcx>> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();

        for origin in origins {
            let candidate = match origin {
                CallableOrigin::Function(instance) => instance,
                CallableOrigin::Closure { def_id, args } => {
                    let instance = if let Some(kind) = requested_kind {
                        Instance::resolve_closure(self.tcx, def_id, args, kind)
                    } else {
                        Instance::new_raw(def_id, args)
                    };
                    FunctionInstance::new_instance(instance)
                }
            };

            if seen.insert(candidate) {
                out.push(candidate);
            }
        }

        out
    }

    fn trace_origins(&self, operand: &mir::Operand<'tcx>) -> Vec<CallableOrigin<'tcx>> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.collect_origins_from_operand(operand, &mut out, &mut seen);

        let Some(start_place) = operand_place(operand).and_then(TrackedPlace::from_place) else {
            return out;
        };

        let predecessors = self.caller_body.basic_blocks.predecessors();
        let mut worklist = VecDeque::from([(self.current_bb, start_place, usize::MAX)]);
        let mut visited = HashSet::new();

        while let Some((bb, tracked, limit)) = worklist.pop_front() {
            if !visited.insert((bb, tracked, limit)) {
                continue;
            }

            let bb_data = &self.caller_body.basic_blocks[bb];
            let stmt_limit = limit.min(bb_data.statements.len());
            let mut found_def = false;

            for idx in (0..stmt_limit).rev() {
                let stmt = &bb_data.statements[idx];
                let mir::StatementKind::Assign(assign) = &stmt.kind else {
                    continue;
                };
                let (lhs, rhs) = &**assign;

                if tracked.matches_exact_lhs(*lhs) {
                    found_def = true;
                    self.collect_origins_from_assignment_rhs(tracked, rhs, idx, bb, &mut worklist, &mut out, &mut seen);
                    break;
                }

                if tracked.field.is_some() && tracked.matches_whole_local(*lhs) {
                    found_def = true;
                    self.collect_origins_from_whole_local_assignment(
                        tracked,
                        rhs,
                        idx,
                        bb,
                        &mut worklist,
                        &mut out,
                        &mut seen,
                    );
                    break;
                }
            }

            if found_def {
                continue;
            }

            for pred in predecessors[bb].iter().copied() {
                if tracked.field.is_none()
                    && self.collect_origins_from_predecessor_call(pred, bb, tracked, &mut worklist, &mut out, &mut seen)
                {
                    continue;
                }
                worklist.push_back((pred, tracked, usize::MAX));
            }
        }

        out
    }

    fn collect_origins_from_assignment_rhs(
        &self,
        tracked: TrackedPlace,
        rhs: &mir::Rvalue<'tcx>,
        idx: usize,
        bb: mir::BasicBlock,
        worklist: &mut VecDeque<(mir::BasicBlock, TrackedPlace, usize)>,
        out: &mut Vec<CallableOrigin<'tcx>>,
        seen: &mut HashSet<CallableOrigin<'tcx>>,
    ) {
        match rhs {
            mir::Rvalue::Use(op) | mir::Rvalue::Cast(_, op, _) => {
                self.collect_origins_from_operand(op, out, seen);
                if let Some(next) = follow_operand_place(op, tracked.field) {
                    worklist.push_back((bb, next, idx));
                }
            }
            mir::Rvalue::Ref(_, _, place) => {
                if let Some(next) = follow_place(*place, tracked.field) {
                    worklist.push_back((bb, next, idx));
                }
                self.collect_origins_from_type(place.ty(self.caller_body, self.tcx).ty, out, seen);
            }
            _ => {}
        }
    }

    fn collect_origins_from_whole_local_assignment(
        &self,
        tracked: TrackedPlace,
        rhs: &mir::Rvalue<'tcx>,
        idx: usize,
        bb: mir::BasicBlock,
        worklist: &mut VecDeque<(mir::BasicBlock, TrackedPlace, usize)>,
        out: &mut Vec<CallableOrigin<'tcx>>,
        seen: &mut HashSet<CallableOrigin<'tcx>>,
    ) {
        match rhs {
            mir::Rvalue::Use(op) | mir::Rvalue::Cast(_, op, _) => {
                self.collect_origins_from_operand(op, out, seen);
                if let Some(next) = follow_operand_place(op, tracked.field) {
                    worklist.push_back((bb, next, idx));
                }
            }
            mir::Rvalue::Ref(_, _, place) => {
                if let Some(next) = follow_place(*place, tracked.field) {
                    worklist.push_back((bb, next, idx));
                }
                self.collect_origins_from_type(place.ty(self.caller_body, self.tcx).ty, out, seen);
            }
            mir::Rvalue::Aggregate(_, operands) => {
                let Some(field) = tracked.field else {
                    return;
                };
                let Some(op) = operands.get(field) else {
                    return;
                };
                self.collect_origins_from_operand(op, out, seen);
                if let Some(next) = follow_operand_place(op, None) {
                    worklist.push_back((bb, next, idx));
                }
            }
            _ => {}
        }
    }

    fn collect_origins_from_predecessor_call(
        &self,
        pred: mir::BasicBlock,
        succ: mir::BasicBlock,
        tracked: TrackedPlace,
        worklist: &mut VecDeque<(mir::BasicBlock, TrackedPlace, usize)>,
        out: &mut Vec<CallableOrigin<'tcx>>,
        seen: &mut HashSet<CallableOrigin<'tcx>>,
    ) -> bool {
        let Some(terminator) = self.caller_body.basic_blocks[pred].terminator.as_ref() else {
            return false;
        };

        let mir::TerminatorKind::Call {
            func,
            args,
            destination,
            target,
            ..
        } = &terminator.kind
        else {
            return false;
        };

        if *target != Some(succ) || destination.local != tracked.local || !destination.as_ref().projection.is_empty() {
            return false;
        }

        let Some((callee_def_id, _)) = operand_fn_def(func) else {
            return true;
        };

        match summarize_callable_return(self.tcx, callee_def_id) {
            ReturnSummary::Param(index) => {
                let Some(arg) = args.get(index) else {
                    return true;
                };
                self.collect_origins_from_operand(&arg.node, out, seen);
                if let Some(next) = follow_operand_place(&arg.node, None) {
                    let pred_stmt_len = self.caller_body.basic_blocks[pred].statements.len();
                    worklist.push_back((pred, next, pred_stmt_len));
                }
            }
            ReturnSummary::WrappedParam(index) => {
                let Some(arg) = args.get(index) else {
                    return true;
                };
                self.collect_origins_from_operand(&arg.node, out, seen);
                if let Some(next) = follow_operand_place(&arg.node, None) {
                    let pred_stmt_len = self.caller_body.basic_blocks[pred].statements.len();
                    worklist.push_back((pred, next, pred_stmt_len));
                }
            }
            ReturnSummary::Function { def_id, args } => {
                let origin = CallableOrigin::Function(self.resolve_function_instance(def_id, args));
                if seen.insert(origin) {
                    out.push(origin);
                }
            }
            ReturnSummary::Closure { def_id, args } => {
                let origin = CallableOrigin::Closure { def_id, args };
                if seen.insert(origin) {
                    out.push(origin);
                }
            }
            ReturnSummary::Unknown => {}
        }

        true
    }

    fn collect_origins_from_operand(
        &self,
        operand: &mir::Operand<'tcx>,
        out: &mut Vec<CallableOrigin<'tcx>>,
        seen: &mut HashSet<CallableOrigin<'tcx>>,
    ) {
        if let Some(origin) = self.origin_from_fn_def_operand(operand)
            && seen.insert(origin)
        {
            out.push(origin);
        }

        self.collect_origins_from_type(operand.ty(self.caller_body, self.tcx), out, seen);
    }

    fn origin_from_fn_def_operand(&self, operand: &mir::Operand<'tcx>) -> Option<CallableOrigin<'tcx>> {
        let (def_id, args) = match operand {
            mir::Operand::Constant(c) => match c.ty().kind() {
                ty::TyKind::FnDef(def_id, args) => (*def_id, *args),
                _ => operand.const_fn_def()?,
            },
            _ => operand.const_fn_def()?,
        };
        Some(CallableOrigin::Function(self.resolve_function_instance(def_id, args)))
    }

    fn collect_origins_from_type(
        &self,
        ty: ty::Ty<'tcx>,
        out: &mut Vec<CallableOrigin<'tcx>>,
        seen: &mut HashSet<CallableOrigin<'tcx>>,
    ) {
        match ty.kind() {
            ty::TyKind::FnDef(def_id, args) => {
                let origin = CallableOrigin::Function(self.resolve_function_instance(*def_id, *args));
                if seen.insert(origin) {
                    out.push(origin);
                }
            }
            ty::TyKind::Closure(def_id, args) => {
                let origin = CallableOrigin::Closure {
                    def_id: *def_id,
                    args: *args,
                };
                if seen.insert(origin) {
                    out.push(origin);
                }
            }
            ty::TyKind::Ref(_, inner, _) => self.collect_origins_from_type(*inner, out, seen),
            ty::TyKind::RawPtr(inner, _) => self.collect_origins_from_type(*inner, out, seen),
            _ if ty.is_box_global(self.tcx) => self.collect_origins_from_type(ty.expect_boxed_ty(), out, seen),
            ty::TyKind::Adt(_, args) => {
                for arg in args.iter() {
                    if let Some(inner) = arg.as_type() {
                        self.collect_origins_from_type(inner, out, seen);
                    }
                }
            }
            _ => {}
        }
    }

    fn resolve_function_instance(&self, def_id: DefId, args: ty::GenericArgsRef<'tcx>) -> FunctionInstance<'tcx> {
        match ty::Instance::try_resolve(self.tcx, self.typing_env, def_id, args) {
            Ok(Some(instance)) => FunctionInstance::new_instance(instance),
            _ => trivial_resolve(self.tcx, def_id).unwrap_or(FunctionInstance::new_non_instance(def_id)),
        }
    }
}

fn operand_place<'tcx>(operand: &mir::Operand<'tcx>) -> Option<mir::Place<'tcx>> {
    match operand {
        mir::Operand::Move(place) | mir::Operand::Copy(place) => Some(*place),
        mir::Operand::Constant(_) => None,
    }
}

fn operand_fn_def<'tcx>(operand: &mir::Operand<'tcx>) -> Option<(DefId, ty::GenericArgsRef<'tcx>)> {
    match operand {
        mir::Operand::Constant(c) => match c.ty().kind() {
            ty::TyKind::FnDef(def_id, args) => Some((*def_id, *args)),
            _ => operand.const_fn_def(),
        },
        _ => operand.const_fn_def(),
    }
}

fn follow_operand_place<'tcx>(operand: &mir::Operand<'tcx>, field: Option<FieldIdx>) -> Option<TrackedPlace> {
    let place = operand_place(operand)?;
    follow_place(place, field)
}

fn follow_place(place: mir::Place<'_>, field: Option<FieldIdx>) -> Option<TrackedPlace> {
    let tracked = TrackedPlace::from_place(place)?;
    if tracked.field.is_some() {
        Some(tracked)
    } else {
        Some(TrackedPlace {
            local: tracked.local,
            field,
        })
    }
}
