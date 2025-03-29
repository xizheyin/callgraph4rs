#![feature(rustc_private)]

use cg4rs::CGDriver;
use rustc_compat::rustc_main;

fn main() {
    tracing_subscriber::fmt::init();
    tracing::trace!("run cg4rs");
    rustc_main(CGDriver);
}
