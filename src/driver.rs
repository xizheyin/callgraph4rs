use clap::Parser;
use owo_colors::OwoColorize;
use rustc_driver::Compilation;
use rustc_interface::{interface, Queries};
use std::borrow::Cow;
use std::env;
use std::process::Command;
use std::str;

use crate::args::{AllCliArgs, CGArgs};
use crate::callgraph;
use rustc_compat::{CrateFilter, Plugin, RustcPluginArgs, Utf8Path};

#[derive(Default)]
pub struct CGDriver;

impl Plugin for CGDriver {
    type CargoArgs = Vec<String>;
    type PluginArgs = CGArgs;

    fn version(&self) -> Cow<'static, str> {
        env!("CARGO_PKG_VERSION").into()
    }

    fn driver_name(&self) -> Cow<'static, str> {
        "cg".into()
    }

    // In the CLI, we ask Clap to parse arguments and also specify a CrateFilter.
    // If one of the CLI arguments was a specific file to analyze, then you
    // could provide a different filter.
    fn args(&self, _target_dir: &Utf8Path) -> RustcPluginArgs<Self::CargoArgs, Self::PluginArgs> {
        let args = AllCliArgs::parse_from(env::args().skip(1));
        let filter = CrateFilter::AllCrates;
        RustcPluginArgs {
            cargo_args: args.cargo_args,
            plugin_args: args.cg_args,
            filter,
        }
    }

    // Pass Cargo arguments (like --feature) from the top-level CLI to Cargo.
    fn modify_cargo(&self, cargo: &mut Command, cargo_args: &Self::CargoArgs) {
        cargo.args(cargo_args);
    }

    // In the driver, we use the Rustc API to start a compiler session
    // for the arguments given to us by rustc_plugin.
    fn run(
        self,
        compiler_args: Vec<String>,
        plugin_args: Self::PluginArgs,
    ) -> rustc_interface::interface::Result<()> {
        tracing::debug!("Rust CG start to run.");
        let mut callbacks = CGCallbacks::new(&plugin_args);
        let compiler = rustc_driver::RunCompiler::new(&compiler_args, &mut callbacks);
        compiler.run()
    }
}

pub(crate) struct CGCallbacks {
    args: CGArgs,
}

impl CGCallbacks {
    pub(crate) fn new(args: &CGArgs) -> Self {
        Self { args: args.clone() }
    }
}

impl rustc_driver::Callbacks for CGCallbacks {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        queries: &'tcx Queries<'tcx>,
    ) -> Compilation {
        tracing::info!("{}", "Entering after_analysis callback".red());
        queries.global_ctxt().unwrap().enter(|tcx| {
            let generic_instances = callgraph::collect_generic_instances(tcx);
            let call_graph = callgraph::perform_mono_analysis(tcx, generic_instances);
            println!("call_graph: {:#?}", call_graph.call_sites);
        });
        tracing::info!("{}", "Exiting after_analysis callback".red());
        Compilation::Continue
    }
}
