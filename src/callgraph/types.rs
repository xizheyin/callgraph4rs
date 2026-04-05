use std::collections::VecDeque;

use super::function::FunctionInstance;

pub(crate) struct CallGraph<'tcx> {
    pub(crate) instances: VecDeque<FunctionInstance<'tcx>>,
    pub(crate) call_sites: Vec<CallSite<'tcx>>,
    pub(crate) without_args: bool,
    pub(crate) total_functions: usize,
}

impl<'tcx> CallGraph<'tcx> {
    pub(crate) fn new(all_generic_instances: Vec<FunctionInstance<'tcx>>, without_args: bool) -> Self {
        Self {
            instances: all_generic_instances.into_iter().collect(),
            call_sites: Vec::new(),
            without_args,
            total_functions: 0,
        }
    }
}

/// Represents a call site in the code
#[derive(Debug, Clone)]
pub struct CallSite<'tcx> {
    caller: FunctionInstance<'tcx>,
    callee: FunctionInstance<'tcx>,
    constraint_cnt: usize,
    call_kind: CallKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallKind {
    Direct,
    FnPtr,
    DynTrait,
    Drop,
}

impl<'tcx> CallSite<'tcx> {
    /// Create a new CallSite
    pub fn new(caller: FunctionInstance<'tcx>, callee: FunctionInstance<'tcx>, constraint_count: usize) -> Self {
        Self {
            caller,
            callee,
            constraint_cnt: constraint_count,
            call_kind: CallKind::Direct,
        }
    }

    /// Create a new CallSite with specific call kind
    pub fn new_with_kind(
        caller: FunctionInstance<'tcx>,
        callee: FunctionInstance<'tcx>,
        constraint_count: usize,
        call_kind: CallKind,
    ) -> Self {
        Self {
            caller,
            callee,
            constraint_cnt: constraint_count,
            call_kind,
        }
    }

    /// Get the caller of this call site
    pub fn caller(&self) -> FunctionInstance<'tcx> {
        self.caller
    }

    /// Get the callee of this call site
    pub fn callee(&self) -> FunctionInstance<'tcx> {
        self.callee
    }

    /// Get the constraint count of this call site
    pub fn constraint_count(&self) -> usize {
        self.constraint_cnt
    }

    pub fn package_num(&self) -> usize {
        if self.caller.def_id().krate == self.callee.def_id().krate {
            0
        } else {
            1
        }
    }

    pub fn call_kind(&self) -> CallKind {
        self.call_kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathInfo<'tcx> {
    pub(crate) caller: FunctionInstance<'tcx>,
    pub(crate) call_path: Vec<FunctionInstance<'tcx>>,
    pub(crate) constraints: usize,
    pub(crate) package_num: usize,
    pub(crate) package_num_unique: usize,
    pub(crate) path_len: usize,
    pub(crate) dyn_edges: usize,
    pub(crate) fnptr_edges: usize,
    pub(crate) generic_args_len_sum: usize,
}

impl<'tcx> PartialOrd for PathInfo<'tcx> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'tcx> Ord for PathInfo<'tcx> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a_name = format!("{:?}", self.caller);
        let b_name = format!("{:?}", other.caller);

        match a_name.cmp(&b_name) {
            std::cmp::Ordering::Equal => match self.constraints.cmp(&other.constraints) {
                std::cmp::Ordering::Equal => match self.package_num.cmp(&other.package_num) {
                    std::cmp::Ordering::Equal => self.path_len.cmp(&other.path_len),
                    non_eq => non_eq,
                },
                non_eq => non_eq,
            },
            non_eq => non_eq,
        }
    }
}
