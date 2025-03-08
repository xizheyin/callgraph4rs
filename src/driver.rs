use clap::Parser;
use owo_colors::OwoColorize;
use rustc_driver::Compilation;
use rustc_interface::{interface, Queries};
use std::borrow::Cow;
use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;

use crate::args::{AllCliArgs, CGArgs};
use crate::callgraph;
use rustc_compat::{CrateFilter, Plugin, RustcPluginArgs, Utf8Path};

/// 将内容写入指定文件并记录日志
///
/// # 参数
/// * `path` - 要写入的文件路径
/// * `write_fn` - 负责写入内容的闭包
///
/// # 返回值
/// * `io::Result<()>` - 操作成功返回 Ok(()), 失败返回 Err
fn write_to_file<P, F>(path: P, write_fn: F) -> io::Result<()>
where
    P: AsRef<Path>,
    F: FnOnce(&mut std::fs::File) -> io::Result<()>,
{
    // 确保父目录存在
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 创建和写入文件
    let mut file = std::fs::File::create(&path)?;
    write_fn(&mut file)?;

    // 记录成功信息
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
    fn run(
        self,
        compiler_args: Vec<String>,
        _plugin_args: Self::PluginArgs,
    ) -> rustc_interface::interface::Result<()> {
        tracing::debug!("Rust CG start to run.");
        let mut callbacks = CGCallbacks {};
        let compiler = rustc_driver::RunCompiler::new(&compiler_args, &mut callbacks);
        compiler.run()
    }
}

pub(crate) struct CGCallbacks {}

impl rustc_driver::Callbacks for CGCallbacks {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &interface::Compiler,
        queries: &'tcx Queries<'tcx>,
    ) -> Compilation {
        tracing::info!("{}", "Entering after_analysis callback");
        queries.global_ctxt().unwrap().enter(|tcx| {
            let generic_instances = callgraph::collect_generic_instances(tcx);
            let call_graph = callgraph::perform_mono_analysis(tcx, generic_instances);

            // 使用抽象函数写入调用图
            let output_path = PathBuf::from("target/callgraph.txt");

            match write_to_file(&output_path, |file| {
                writeln!(file, "call_graph: {:#?}", call_graph.call_sites)
            }) {
                Ok(_) => tracing::info!("Call graph written to {}", output_path.display()),
                Err(e) => tracing::error!("Failed to write call graph: {}", e),
            }
        });
        tracing::info!("{}", "Exiting after_analysis callback");
        Compilation::Continue
    }
}
