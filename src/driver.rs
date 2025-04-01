use clap::Parser;
use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;
use std::borrow::Cow;
use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::str;

use crate::args::{AllCliArgs, CGArgs};
use crate::callgraph;
use crate::timer::Timer;
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
        "cg4rs".into()
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
        // Set up timer output file
        let timer_output_path = if let Some(timer_path) = &plugin_args.timer_output {
            timer_path.clone()
        } else {
            plugin_args
                .output_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from("./target"))
                .join("cg_timing.txt")
        };

        Timer::set_output_file(
            timer_output_path
                .to_str()
                .unwrap_or("./target/cg_timing.txt"),
        );

        // Start overall timer
        Timer::start("Overall_execution");

        tracing::debug!("Rust CG start to run.");
        let mut callbacks = CGCallbacks::new(plugin_args);

        // Record rustc_driver execution time
        rustc_driver::run_compiler(&compiler_args, &mut callbacks);

        // Stop overall timer and write results to file
        Timer::stop("Overall_execution");
        if let Err(e) = Timer::write_to_file() {
            tracing::error!("Failed to write timer results to file: {:?}", e);
        }
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
