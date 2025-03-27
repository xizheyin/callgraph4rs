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
}
