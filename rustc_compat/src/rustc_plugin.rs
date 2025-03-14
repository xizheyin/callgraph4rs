use std::{
    env,
    ops::Deref,
    path::{Path, PathBuf},
    process::{exit, Command},
};

use super::plugin::{Plugin, PLUGIN_ARGS};
use crate::cargo_plugin::{RUN_ON_ALL_CRATES, SPECIFIC_CRATE, SPECIFIC_TARGET};
use rustc_session::{config::ErrorOutputType, EarlyDiagCtxt};

/// 来自 clippy
/// 如果命令行选项与 `arg_to_be_found` 匹配，则对其值应用谓词 `pred`。如果为真，则返回该值，否则返回None。
/// 参数假定为 `--arg=value` 或 `--arg value` 格式。
fn arg_value<'a, T: Deref<Target = str>>(
    args: &'a [T],
    find_arg: &str,
    pred: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    let mut args = args.iter().map(Deref::deref);
    while let Some(arg) = args.next() {
        let mut arg = arg.splitn(2, '=');
        if arg.next() != Some(find_arg) {
            continue;
        }

        match arg.next().or_else(|| args.next()) {
            Some(v) if pred(v) => return Some(v),
            _ => {}
        }
    }
    None
}

fn toolchain_path(home: Option<String>, toolchain: Option<String>) -> Option<PathBuf> {
    home.and_then(|home| {
        toolchain.map(|toolchain| {
            let mut path = PathBuf::from(home);
            path.push("toolchains");
            path.push(toolchain);
            path
        })
    })
}

// 获取系统根目录，按照下面的顺序
// - 命令行
// - 运行时环境变量
//    - SYSROOT
//    - RUSTUP_HOME, RUSTUP_TOOLCHAIN
// - rustc打印的sysroot
// - 编译时环境变量
//    - SYSROOT
//    - RUSTUP_HOME, RUSTUP_TOOLCHAIN
fn get_sysroot(orig_args: &[String]) -> (bool, String) {
    let sys_root_arg = arg_value(orig_args, "--sysroot", |_| true);
    let have_sys_root_arg = sys_root_arg.is_some();
    let sys_root = sys_root_arg
        .map(PathBuf::from)
        .or_else(|| std::env::var("SYSROOT").ok().map(PathBuf::from))
        .or_else(|| {
            let home = std::env::var("RUSTUP_HOME").ok();
            let toolchain = std::env::var("RUSTUP_TOOLCHAIN").ok();
            toolchain_path(home, toolchain)
        })
        .or_else(|| {
            Command::new("rustc")
                .arg("--print")
                .arg("sysroot")
                .output()
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|s| PathBuf::from(s.trim()))
        })
        .or_else(|| option_env!("SYSROOT").map(PathBuf::from))
        .or_else(|| {
            let home = option_env!("RUSTUP_HOME").map(ToString::to_string);
            let toolchain = option_env!("RUSTUP_TOOLCHAIN").map(ToString::to_string);
            toolchain_path(home, toolchain)
        })
        .map(|pb| pb.to_string_lossy().to_string())
        .expect(
            "need to specify SYSROOT env var during clippy compilation, or use rustup or multirust",
        );
    (have_sys_root_arg, sys_root)
}

struct DefaultCallbacks;
impl rustc_driver::Callbacks for DefaultCallbacks {}

/// 包装rustc的调用器。
pub fn rustc_main<T: Plugin>(plugin: T) {
    // 标准流程，早期错误处理
    let early_dcx = EarlyDiagCtxt::new(ErrorOutputType::default());
    rustc_driver::init_rustc_env_logger(&early_dcx);

    exit(rustc_driver::catch_with_exit_code(move || {
        let mut orig_args: Vec<String> = env::args().collect();

        let (have_sys_root_arg, sys_root) = get_sysroot(&orig_args);

        if orig_args.iter().any(|a| a == "--version" || a == "-V") {
            let version_info = rustc_tools_util::get_version_info!();
            println!("{version_info}");
            exit(0);
        }

        // 设置 RUSTC_WRAPPER 会导致 Cargo 传递 'rustc' 作为第一个参数。
        // 我们自动调用编译器，因此忽略这个参数。
        // 检查是否在 RUSTC_WRAPPER 模式下

        if orig_args.get(1).map(Path::new).and_then(Path::file_stem) == Some("rustc".as_ref()) {
            // we still want to be able to invoke it normally though
            orig_args.remove(1);
        }

        // 此条件检查 --sysroot 标志，以便用户可以直接调用驱动程序而无需传递 --sysroot 或其他参数。
        let mut args: Vec<String> = orig_args.clone();
        if !have_sys_root_arg {
            args.extend(["--sysroot".into(), sys_root]);
        };

        // 在一次 rustc 调用中，我们必须决定是作为 rustc 运行，还是实际执行插件。
        // 执行插件有两个条件：
        // 1. 要么我们应该运行所有 crate，或者设置了 CARGO_PRIMARY_PACKAGE。
        // 2. 没有传递 --print，因为 Cargo 会这样做以获取 rustc 的信息。
        let primary_package = env::var("CARGO_PRIMARY_PACKAGE").is_ok();
        let run_on_all_crates = env::var(RUN_ON_ALL_CRATES).is_ok();
        let normal_rustc = arg_value(&args, "--print", |_| true).is_some();
        let is_target_crate = match (env::var(SPECIFIC_CRATE), env::var(SPECIFIC_TARGET)) {
            (Ok(krate), Ok(target)) => {
                arg_value(&args, "--crate-name", |name| name == krate).is_some()
                    && arg_value(&args, "--crate-type", |name| name == target).is_some()
            }
            _ => true,
        };
        let run_plugin = !normal_rustc && (run_on_all_crates || primary_package) && is_target_crate;

        if run_plugin {
            let plugin_args: T::PluginArgs =
                serde_json::from_str(&env::var(PLUGIN_ARGS).unwrap()).unwrap();
            plugin.run(args, plugin_args);
        } else {
            rustc_driver::run_compiler(&args, &mut DefaultCallbacks);
        }
    }))
}
