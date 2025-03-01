//! A framework for writing plugins that integrate with the Rust compiler.
//!
//! Much of this library is either directly copy/pasted, or otherwise generalized
//! from the Clippy driver: <https://github.com/rust-lang/rust-clippy/tree/master/src>

#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_session;

#[doc(hidden)]
pub use cargo_metadata::camino::Utf8Path;
pub use cargo_plugin::cargo_main;
pub use plugin::{CrateFilter, Plugin, RustcPluginArgs};
pub use rustc_plugin::rustc_main;

mod cargo_plugin;
mod plugin;
mod rustc_plugin;
