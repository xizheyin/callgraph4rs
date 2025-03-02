use clap::Parser;
use owo_colors::OwoColorize;
use rustc_driver::Compilation;
use rustc_interface::{interface, Queries};
use rustc_middle::ty::{TyCtxt, TyKind};
use std::borrow::Cow;
use std::env;
use std::process::Command;
use std::str;

use crate::args::{AllCliArgs, CGArgs};
use crate::ccg::{self, output_dependencies_to_target};
use crate::context::Context;
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

    /// print
    fn _print_basic(&mut self, context: &mut Context) {
        let tcx: TyCtxt<'_> = context.tcx;
        for (did, name) in &context._all_generic_funcs_did_sym_map {
            // 打印模块名

            let module_name = tcx.def_path(*did).to_string_no_crate_verbose();
            println!(
                "\nModule Name: {}, Function Name: {}",
                module_name.yellow(),
                name
            );

            let mir = tcx.optimized_mir(did.as_local().unwrap());
            if self.args.show_all_mir {
                //self.visit_body(mir);
                for (bbidx, bb) in mir.basic_blocks.iter_enumerated() {
                    println!("BasicBlock {}:", format!("{:?}", bbidx).red());

                    for stmt in bb.statements.iter() {
                        println!("{:?}", stmt);
                    }
                    println!("{:?}", bb.terminator()); // terminator
                    match &bb.terminator().kind {
                        rustc_middle::mir::TerminatorKind::Call { func, .. } => {
                            println!("Call a function {:?}!", func);
                            match func {
                                rustc_middle::mir::Operand::Constant(const_val) => {
                                    match const_val.ty().kind() {
                                        TyKind::Closure(callee_def_id, _gen_args)
                                        | TyKind::FnDef(callee_def_id, _gen_args)
                                        | TyKind::Coroutine(callee_def_id, _gen_args) => {
                                            println!(
                                                "【Callee: {}】",
                                                tcx.def_path_str(callee_def_id).green(),
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
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
            let context = Box::new(Context::new(tcx, self.args.to_hash_map()));

            // self.print_basic(&mut context);
            // let mir = tcx.optimized_mir(did.as_local().unwrap());

            let all_dependencies = ccg::extract_dependencies(context.tcx);
            let _ = output_dependencies_to_target(tcx, all_dependencies);
        });
        tracing::info!("{}", "Exiting after_analysis callback".red());
        Compilation::Continue
    }
}
