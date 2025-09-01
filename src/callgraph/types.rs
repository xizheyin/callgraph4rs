use super::function::FunctionInstance;

/// Represents a call site in the code
#[derive(Debug, Clone)]
pub struct CallSite<'tcx> {
    caller: FunctionInstance<'tcx>,
    callee: FunctionInstance<'tcx>,
    constraint_cnt: usize,
}

impl<'tcx> CallSite<'tcx> {
    /// Create a new CallSite
    pub fn new(
        caller: FunctionInstance<'tcx>,
        callee: FunctionInstance<'tcx>,
        constraint_count: usize,
    ) -> Self {
        Self {
            caller,
            callee,
            constraint_cnt: constraint_count,
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
            1
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathInfo<'tcx> {
    pub(crate) caller: FunctionInstance<'tcx>,
    pub(crate) constraints: usize,
    pub(crate) package_num: usize,
}

impl<'tcx> PartialOrd for PathInfo<'tcx> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let a_name = format!("{:?}", self.caller);
        let b_name = format!("{:?}", other.caller);

        match a_name.partial_cmp(&b_name) {
            Some(std::cmp::Ordering::Equal) => {
                match self.constraints.partial_cmp(&other.constraints) {
                    Some(std::cmp::Ordering::Equal) => {
                        self.package_num.partial_cmp(&other.package_num)
                    }
                    non_eq => non_eq,
                }
            }
            non_eq => non_eq,
        }
    }
}

impl<'tcx> Ord for PathInfo<'tcx> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a_name = format!("{:?}", self.caller);
        let b_name = format!("{:?}", other.caller);

        match a_name.cmp(&b_name) {
            std::cmp::Ordering::Equal => match self.constraints.cmp(&other.constraints) {
                std::cmp::Ordering::Equal => self.package_num.cmp(&other.package_num),
                non_eq => non_eq,
            },
            non_eq => non_eq,
        }
    }
}
