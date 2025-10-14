//! Utilities for analyzing MIR control flow.

use rustc_hir::def_id::DefId;
use rustc_middle::{
    mir::{self, BasicBlock, TerminatorKind},
    ty::TyCtxt,
};
use std::collections::{HashMap, VecDeque};

/// Types of constraints that can appear in MIR
/// FIXME: Add other types of constraints
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintKind {
    /// Switch on an integer value (if/match)
    SwitchInt,
    /// Other types of constraints
    _Other(String),
}

impl ConstraintKind {
    /// Determines if a terminator represents a conditional constraint
    pub fn from_terminator(terminator: &TerminatorKind<'_>) -> Option<Self> {
        match terminator {
            TerminatorKind::SwitchInt { .. } => Some(ConstraintKind::SwitchInt),
            //TerminatorKind::Assert { .. } => Some(ConstraintKind::Assert),
            _ => None,
        }
    }
}

/// Represents a path through basic blocks with constraint tracking
#[derive(Debug, Clone)]
pub struct BlockPath {
    /// Sequence of basic blocks forming the path
    pub blocks: Vec<BasicBlock>,
    /// Number of conditional constraints along the path
    pub constraints: usize,
}

impl BlockPath {
    /// Creates a new path with a single basic block
    fn new(block: BasicBlock) -> Self {
        BlockPath {
            blocks: vec![block],
            constraints: 0,
        }
    }

    /// Extends the path with a new basic block, tracking the path
    fn extend(&self, block: BasicBlock, is_constraint: bool) -> Self {
        let mut blocks = self.blocks.clone();
        let mut constraints = self.constraints;

        blocks.push(block);

        // If this extension involves a constraint, track it
        if is_constraint {
            constraints += 1;
        }

        BlockPath {
            blocks,
            constraints,
        }
    }
}

/// Computes the shortest paths from the entry block to all other blocks
/// with constraint tracking
///
/// # Arguments
/// * `body` - The MIR body to analyze
///
/// # Returns
/// * A map from each basic block to its shortest path from the entry block
pub fn compute_shortest_paths(tcx: TyCtxt<'_>, def_id: DefId) -> HashMap<BasicBlock, BlockPath> {
    let body = tcx.optimized_mir(def_id);
    let entry = mir::START_BLOCK;
    let mut result: HashMap<BasicBlock, BlockPath> = HashMap::new();
    let mut best_constraints: HashMap<BasicBlock, usize> = HashMap::new();
    let mut deque: VecDeque<BasicBlock> = VecDeque::new();

    // initialize: entry block has 0 constraints
    result.insert(entry, BlockPath::new(entry));
    best_constraints.insert(entry, 0);
    deque.push_front(entry);

    // 0-1 BFS: find the path with the fewest constraints (edge weights 0 or 1)
    while let Some(block) = deque.pop_front() {
        let current_path = result[&block].clone();
        let current_cost = best_constraints[&block];

        // Process all successors of the current block
        if let Some(terminator) = body.basic_blocks[block].terminator.as_ref() {
            // Current edge weight: 1 if it's a constraint, 0 otherwise
            let is_constraint_edge = ConstraintKind::from_terminator(&terminator.kind).is_some();
            let edge_weight = if is_constraint_edge { 1 } else { 0 };

            for target in terminator.successors() {
                let next_cost = current_cost + edge_weight;

                // if target has no record or we found a path with fewer constraints, update
                match best_constraints.get(&target) {
                    Some(&best) if next_cost >= best => {}
                    _ => {
                        let new_path = current_path.extend(target, is_constraint_edge);
                        result.insert(target, new_path);
                        best_constraints.insert(target, next_cost);

                        // 0-1 BFS: 0-weight edges go to front, 1-weight edges go to back
                        if edge_weight == 0 {
                            deque.push_front(target);
                        } else {
                            deque.push_back(target);
                        }
                    }
                }
            }
        }
    }

    result
}
