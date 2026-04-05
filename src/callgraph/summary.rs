use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{self, AggregateKind},
    ty::{self, TyCtxt},
};
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ReturnSummary<'tcx> {
    Param(usize),
    WrappedParam(usize),
    Function {
        def_id: DefId,
        args: ty::GenericArgsRef<'tcx>,
    },
    Closure {
        def_id: DefId,
        args: ty::GenericArgsRef<'tcx>,
    },
    Unknown,
}

pub(crate) fn summarize_callable_return<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> ReturnSummary<'tcx> {
    if let Some(summary) = builtin_callable_producer_summary(tcx, def_id) {
        return summary;
    }

    if !tcx.is_mir_available(def_id) {
        return ReturnSummary::Unknown;
    }

    let body = tcx.optimized_mir(def_id);
    let predecessors = body.basic_blocks.predecessors();
    let mut summaries = HashSet::new();

    for bb in body.basic_blocks.indices() {
        let Some(terminator) = body.basic_blocks[bb].terminator.as_ref() else {
            continue;
        };
        if !matches!(terminator.kind, mir::TerminatorKind::Return) {
            continue;
        }

        let mut worklist = VecDeque::from([(bb, mir::RETURN_PLACE, body.basic_blocks[bb].statements.len())]);
        let mut visited = HashSet::new();

        while let Some((bb, local, limit)) = worklist.pop_front() {
            if !visited.insert((bb, local, limit)) {
                continue;
            }

            if let Some(param_idx) = param_index(body, local) {
                summaries.insert(ReturnSummary::Param(param_idx));
                continue;
            }

            let bb_data = &body.basic_blocks[bb];
            let stmt_limit = limit.min(bb_data.statements.len());
            let mut found_def = false;

            for idx in (0..stmt_limit).rev() {
                let stmt = &bb_data.statements[idx];
                let mir::StatementKind::Assign(assign) = &stmt.kind else {
                    continue;
                };
                let (lhs, rhs) = &**assign;
                if lhs.local != local || !lhs.as_ref().projection.is_empty() {
                    continue;
                }

                found_def = true;
                match rhs {
                    mir::Rvalue::Use(op) | mir::Rvalue::Cast(_, op, _) => {
                        if let Some(summary) = summary_from_operand(op, body) {
                            summaries.insert(summary);
                        } else if let Some(place) = operand_place(op)
                            && place.as_ref().projection.is_empty()
                        {
                            worklist.push_back((bb, place.local, idx));
                        }
                    }
                    mir::Rvalue::Aggregate(kind, _) => match &**kind {
                        AggregateKind::Closure(def_id, args) => {
                            summaries.insert(ReturnSummary::Closure {
                                def_id: *def_id,
                                args: *args,
                            });
                        }
                        _ => {
                            summaries.insert(ReturnSummary::Unknown);
                        }
                    },
                    _ => {
                        summaries.insert(ReturnSummary::Unknown);
                    }
                }
                break;
            }

            if found_def {
                continue;
            }

            for pred in predecessors[bb].iter().copied() {
                worklist.push_back((pred, local, body.basic_blocks[pred].statements.len()));
            }
        }
    }

    if summaries.len() == 1 {
        summaries.into_iter().next().unwrap_or(ReturnSummary::Unknown)
    } else {
        ReturnSummary::Unknown
    }
}

fn summary_from_operand<'tcx>(operand: &mir::Operand<'tcx>, body: &mir::Body<'tcx>) -> Option<ReturnSummary<'tcx>> {
    if let Some((def_id, args)) = operand.const_fn_def() {
        return Some(ReturnSummary::Function { def_id, args });
    }

    let place = operand_place(operand)?;
    if !place.as_ref().projection.is_empty() {
        return None;
    }

    param_index(body, place.local).map(ReturnSummary::Param)
}

fn operand_place<'tcx>(operand: &mir::Operand<'tcx>) -> Option<mir::Place<'tcx>> {
    match operand {
        mir::Operand::Move(place) | mir::Operand::Copy(place) => Some(*place),
        mir::Operand::Constant(_) => None,
    }
}

fn param_index<'tcx>(body: &mir::Body<'tcx>, local: mir::Local) -> Option<usize> {
    let local_idx = local.as_usize();
    if (1..=body.arg_count).contains(&local_idx) {
        Some(local_idx - 1)
    } else {
        None
    }
}

fn builtin_callable_producer_summary<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> Option<ReturnSummary<'tcx>> {
    let path = tcx.def_path_str(def_id);
    if path.ends_with("alloc::boxed::Box::<T>::new") || path.contains("alloc::boxed::Box::<") && path.ends_with("::new")
    {
        return Some(ReturnSummary::WrappedParam(0));
    }
    None
}
