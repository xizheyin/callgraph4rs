#![feature(rustc_private)]

extern crate rustc_ast_pretty;
extern crate rustc_driver;
extern crate rustc_error_codes;
extern crate rustc_errors;
extern crate rustc_hash;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_infer;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;
extern crate rustc_type_ir;

mod args;
mod callgraph;
mod constraint_utils;
/// the driver for perform analysis and generate report
mod driver;
mod process;
/// timer module for measuring execution time
pub mod timer;

/// driver
pub use driver::CGDriver;

pub use process::{setup_signal_handling, start_process_tree_monitoring};
