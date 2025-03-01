#![feature(rustc_private)]

use cg::CGDriver;
use rustc_compat::rustc_main;

fn main() {
    tracing_subscriber::fmt::init();
    tracing::trace!("run cg");
    rustc_main(CGDriver);
}
