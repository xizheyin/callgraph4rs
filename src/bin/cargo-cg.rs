#![feature(rustc_private)]

use cg::CGDriver;
use rustc_compat::cargo_main;

fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();
    tracing::debug!("run cargo cg");
    cargo_main(CGDriver);
}
