use clap::Parser;
use rustc_driver::Compilation;
use rustc_interface::interface;
use rustc_middle::ty::TyCtxt;
use std::borrow::Cow;
use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;

use crate::args::{AllCliArgs, CGArgs};
use crate::callgraph;
use rustc_compat::{CrateFilter, Plugin, RustcPluginArgs, Utf8Path};

/// Write content to a specified file and log the result
///
/// # Parameters
/// * `path` - Path to the output file
/// * `write_fn` - Closure that handles the actual writing
///
/// # Returns
/// * `io::Result<()>` - Ok(()) on success, Err on failure
fn write_to_file<P, F>(path: P, write_fn: F) -> io::Result<()>
where
    P: AsRef<Path>,
    F: FnOnce(&mut std::fs::File) -> io::Result<()>,
{
    // Ensure parent directory exists
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create and write to the file
    let mut file = std::fs::File::create(&path)?;
    write_fn(&mut file)?;

    // Log success message
    tracing::info!("Successfully wrote to file: {}", path.as_ref().display());
    Ok(())
}

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

        let callgraph = callgraph::analyze_crate(tcx, &self.plugin_args);

        // 获取当前 crate 名称
        let crate_name = tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).to_string();

        // 创建输出文件路径
        let output_dir = self
            .plugin_args
            .output_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("target"));
        let output_path = output_dir.join(format!("{}-callgraph.txt", crate_name));

        // 获取格式化的调用图输出
        let formatted_callgraph = callgraph.format_call_graph(tcx);

        // 写入调用图到文件
        match write_to_file(&output_path, |file| write!(file, "{}", formatted_callgraph)) {
            Ok(_) => tracing::info!("Call graph written to {}", output_path.display()),
            Err(e) => tracing::error!("Failed to write call graph: {}", e),
        }

        // 为了方便调试，也可以创建一个包含原始数据的文件
        let debug_path = output_dir.join(format!("{}-callgraph-debug.txt", crate_name));
        let _ = write_to_file(&debug_path, |file| {
            writeln!(file, "call_graph: {:#?}", callgraph.call_sites)
        });

        tracing::info!("{}", "Exiting after_analysis callback");
        Compilation::Continue
    }
}
