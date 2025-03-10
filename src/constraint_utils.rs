//! Utilities for analyzing MIR control flow.

use rustc_middle::mir::{self, BasicBlock, Body, TerminatorKind};
use std::collections::{HashMap, HashSet, VecDeque};

/// Represents a path through basic blocks with constraint tracking
#[derive(Debug, Clone)]
pub struct BlockPath {
    /// Sequence of basic blocks forming the path
    pub blocks: Vec<BasicBlock>,
    /// Length of the path (number of edges)
    pub length: usize,
    /// Number of conditional constraints along the path
    pub constraints: usize,
    /// Description of each constraint encountered
    pub constraint_details: Vec<ConstraintInfo>,
}

/// Information about a constraint in the path
#[derive(Debug, Clone)]
pub struct ConstraintInfo {
    /// The block containing the constraint
    pub block: BasicBlock,
    /// Type of the constraint
    pub kind: ConstraintKind,
    /// Source code location if available
    pub source_info: Option<String>,
}

/// Types of constraints that can appear in MIR
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintKind {
    /// Switch on an integer value (if/match)
    SwitchInt,
    /// Other types of constraints
    Other(String),
}

impl BlockPath {
    /// Creates a new path with a single basic block
    fn new(block: BasicBlock) -> Self {
        BlockPath {
            blocks: vec![block],
            length: 0,
            constraints: 0,
            constraint_details: Vec::new(),
        }
    }

    /// Extends the path with a new basic block, tracking constraints
    fn extend(&self, block: BasicBlock, kind: Option<ConstraintInfo>) -> Self {
        let mut blocks = self.blocks.clone();
        let mut constraints = self.constraints;
        let mut constraint_details = self.constraint_details.clone();

        blocks.push(block);

        // If this extension involves a constraint, track it
        if let Some(info) = kind {
            constraints += 1;
            constraint_details.push(info);
        }

        BlockPath {
            blocks,
            length: self.length + 1,
            constraints,
            constraint_details,
        }
    }
}

/// Determines if a terminator represents a conditional constraint
fn is_constraint_terminator(terminator: &TerminatorKind<'_>) -> Option<ConstraintKind> {
    match terminator {
        TerminatorKind::SwitchInt { .. } => Some(ConstraintKind::SwitchInt),
        //TerminatorKind::Assert { .. } => Some(ConstraintKind::Assert),
        _ => None,
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
pub fn compute_shortest_paths<'tcx>(body: &Body<'tcx>) -> HashMap<BasicBlock, BlockPath> {
    let entry = mir::START_BLOCK;
    let mut result = HashMap::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    // Start with the entry block
    result.insert(entry, BlockPath::new(entry));
    visited.insert(entry);
    queue.push_back(entry);

    // BFS to find shortest paths
    while let Some(block) = queue.pop_front() {
        let current_path = result[&block].clone();

        // Process each successor of the current block
        if let Some(terminator) = body.basic_blocks[block].terminator.as_ref() {
            // Check if this terminator represents a constraint
            let constraint_kind = is_constraint_terminator(&terminator.kind);

            // Process each successor
            for target in terminator.successors() {
                if !visited.contains(&target) {
                    // Create constraint info if this is a conditional jump
                    let constraint_info = constraint_kind.as_ref().map(|kind| {
                        let source_str = format!("{:?}", terminator.source_info);
                        ConstraintInfo {
                            block,
                            kind: kind.clone(),
                            source_info: Some(source_str),
                        }
                    });

                    let new_path = current_path.extend(target, constraint_info);
                    result.insert(target, new_path);
                    visited.insert(target);
                    queue.push_back(target);
                }
            }
        }
    }

    result
}
