use clap::Parser;
use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;
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
    fn run(self, compiler_args: Vec<String>, plugin_args: Self::PluginArgs) {
        tracing::debug!("Rust CG start to run.");
        let mut callbacks = CGCallbacks::new(plugin_args);
        rustc_driver::run_compiler(&compiler_args, &mut callbacks);
    }
}

pub(crate) struct CGCallbacks {
    plugin_args: CGArgs,
}

impl CGCallbacks {
    pub fn new(plugin_args: CGArgs) -> Self {
        Self { plugin_args }
    }
}

impl rustc_driver::Callbacks for CGCallbacks {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        tracing::info!("{}", "Entering after_analysis callback");

        callgraph::analyze_crate(tcx, &self.plugin_args);

        tracing::info!("{}", "Exiting after_analysis callback");
        Compilation::Continue
    }
}
