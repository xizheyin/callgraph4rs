use super::plugin::{Plugin, PLUGIN_ARGS};
use crate::CrateFilter;
use cargo_metadata::camino::Utf8Path;
use cargo_util_schemas::manifest::PackageName;
use std::ops::Index;
use std::{
    env, fs,
    path::PathBuf,
    process::{exit, Command, Stdio},
};

// 环境变量常量，用于控制插件的行为
pub const RUN_ON_ALL_CRATES: &str = "RUSTC_PLUGIN_ALL_TARGETS";
pub const SPECIFIC_CRATE: &str = "SPECIFIC_CRATE";
pub const SPECIFIC_TARGET: &str = "SPECIFIC_TARGET";
pub const CARGO_VERBOSE: &str = "CARGO_VERBOSE";

/// 用户命令行工具的主函数
pub fn cargo_main<T: Plugin>(plugin: T) {
    // 检查是否传递了版本参数 `-V`
    if env::args().any(|arg| arg == "-V") {
        println!("{}\nversion={}", plugin.driver_name(), plugin.version());
        return;
    }

    // 构建metadata命令并获取metadata
    tracing::trace!("Fetch metadata");
    let metadata = build_metadata_command().exec().unwrap();

    // 设置插件的目标目录
    let plugin_subdir = format!(
        "plugin-{}",
        option_env!("RUSTC_CHANNEL").unwrap_or("default")
    );
    let target_dir = metadata.target_directory.join(plugin_subdir);

    // 获取插件参数
    let args = plugin.args(&target_dir);

    // 创建 `cargo` 命令
    let mut cmd = Command::new("cargo");
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    // 获取当前可执行文件的路径
    // 即 dir_path/cg4rs
    let mut path = env::current_exe()
        .expect("current executable path invalid")
        .with_file_name(plugin.driver_name().as_ref());
    path.set_extension(
        env::current_exe()
            .unwrap()
            .extension()
            .map(|e| e.to_owned())
            .unwrap_or_default(),
    );

    // 设置环境变量和命令参数
    cmd.env("RUSTC_WORKSPACE_WRAPPER", path)
        .arg("check")
        .arg("--target-dir")
        .arg(&target_dir);

    // 添加manifest-path参数（如果有）
    if let Some(manifest_path) = find_manifest_path() {
        cmd.arg("--manifest-path").arg(manifest_path);
    }

    // 根据环境变量设置 cargo 的输出详细程度
    if env::var(CARGO_VERBOSE).is_ok() {
        cmd.arg("-vv");
    } else {
        cmd.arg("-q");
    }

    // 获取工作区成员
    let workspace_members = metadata
        .workspace_members
        .iter()
        .map(|pkg_id| metadata.index(pkg_id))
        .collect::<Vec<_>>();

    // 根据过滤器类型决定如何运行插件
    match args.filter {
        CrateFilter::CrateContainingFile(file_path) => {
            only_run_on_file(&mut cmd, file_path, &workspace_members, &target_dir);
        }
        CrateFilter::AllCrates | CrateFilter::OnlyWorkspace => {
            cmd.arg("--all");
            match args.filter {
                CrateFilter::AllCrates => {
                    cmd.env(RUN_ON_ALL_CRATES, "");
                }
                CrateFilter::OnlyWorkspace => {}
                CrateFilter::CrateContainingFile(_) => unreachable!(),
            }
        }
    }

    // 将插件参数序列化为 JSON 字符串并设置环境变量
    let args_str = serde_json::to_string(&args.plugin_args).unwrap();
    tracing::debug!("{PLUGIN_ARGS}={args_str}");
    cmd.env(PLUGIN_ARGS, args_str);

    // 特殊处理 rustc 代码库的编译
    if workspace_members
        .iter()
        .any(|pkg| pkg.name == PackageName::new("rustc-main".to_string()).unwrap())
    {
        cmd.env("CFG_RELEASE", "");
    }

    // 允许插件修改 cargo 命令
    plugin.modify_cargo(&mut cmd, &args.cargo_args);

    tracing::info!("Start to Exec: {:?}", cmd);
    // 执行 cargo 命令，并根据其退出状态退出程序
    let exit_status = cmd.status().expect("failed to wait for cargo?");
    tracing::info!("Finish to Exec {:?}", cmd);
    exit(exit_status.code().unwrap_or(-1));
}

/// 构建元数据命令
fn build_metadata_command() -> cargo_metadata::MetadataCommand {
    let mut binding = cargo_metadata::MetadataCommand::new();
    let mut cmd = binding
        .no_deps()
        .other_options(["--all-features".to_string(), "--offline".to_string()]);

    if let Some(manifest_path) = find_manifest_path() {
        tracing::info!("Using manifest path: {}", manifest_path);
        cmd = cmd.manifest_path(manifest_path);
    }

    cmd.clone()
}

/// 从命令行参数查找manifest-path
fn find_manifest_path() -> Option<String> {
    let args: Vec<String> = env::args().skip(1).collect();

    for i in 0..args.len() {
        if args[i].starts_with("--manifest-path=") {
            // 形式: --manifest-path=/path/to/Cargo.toml
            let parts: Vec<&str> = args[i].splitn(2, '=').collect();
            if parts.len() == 2 {
                return Some(parts[1].to_string());
            }
        } else if args[i] == "--manifest-path" && i + 1 < args.len() {
            // 形式: --manifest-path /path/to/Cargo.toml
            return Some(args[i + 1].clone());
        }
    }

    None
}

/// 仅对特定文件所在的 crate 运行插件
fn only_run_on_file(
    cmd: &mut Command,
    file_path: PathBuf,
    workspace_members: &[&cargo_metadata::Package],
    target_dir: &Utf8Path,
) {
    // 规范化文件路径，确保一致性
    let file_path = file_path.canonicalize().unwrap();

    // 查找与文件路径匹配的包和目标
    let mut matching = workspace_members
        .iter()
        .filter_map(|pkg| {
            let targets = pkg
                .targets
                .iter()
                .filter(|target| {
                    let src_path = target.src_path.canonicalize().unwrap();
                    tracing::trace!("Package {} has src path {}", pkg.name, src_path.display());
                    file_path.starts_with(src_path.parent().unwrap())
                })
                .collect::<Vec<_>>();

            let target = (match targets.len() {
                0 => None,
                1 => Some(targets[0]),
                _ => {
                    // 如果有多个匹配的目标，尝试根据文件名匹配
                    let stem = file_path.file_stem().unwrap().to_string_lossy();
                    let name_matches_stem = targets
                        .clone()
                        .into_iter()
                        .find(|target| target.name == stem);

                    // 特殊处理 main.rs 对应的二进制目标
                    name_matches_stem.or_else(|| {
                        let only_bin = targets
                            .iter()
                            .all(|target| !target.kind.contains(&"lib".into()));
                        if only_bin {
                            targets
                                .into_iter()
                                .find(|target| target.kind.contains(&"bin".into()))
                        } else {
                            let kind = (if stem == "main" { "bin" } else { "lib" }).to_string();
                            targets.into_iter().find(|target| {
                                target
                                    .kind
                                    .contains(&cargo_metadata::TargetKind::from(kind.as_str()))
                            })
                        }
                    })
                }
            })?;

            Some((pkg, target))
        })
        .collect::<Vec<_>>();

    // 确保找到唯一的匹配目标
    let (pkg, target) = match matching.len() {
        0 => panic!("Could not find target for path: {}", file_path.display()),
        1 => matching.remove(0),
        _ => panic!("Too many matching targets: {matching:?}"),
    };

    // 设置编译过滤器，指定目标
    cmd.arg("-p").arg(format!("{}:{}", pkg.name, pkg.version));

    enum CompileKind {
        Lib,
        Bin,
        ProcMacro,
    }

    // 根据目标类型设置编译选项
    let kind_str = &target.kind[0].to_string();
    let kind = match kind_str.as_str() {
        "lib" | "rlib" | "dylib" | "staticlib" | "cdylib" => CompileKind::Lib,
        "bin" => CompileKind::Bin,
        "proc-macro" => CompileKind::ProcMacro,
        _ => unreachable!("unexpected cargo crate type: {kind_str}"),
    };

    match kind {
        CompileKind::Lib => {
            // 如果之前生成过库的元数据文件，删除它们以避免缓存问题
            let deps_dir = target_dir.join("debug").join("deps");
            if let Ok(entries) = fs::read_dir(deps_dir) {
                let prefix = format!("lib{}", pkg.name.replace('-', "_"));
                for entry in entries {
                    let path = entry.unwrap().path();
                    if let Some(file_name) = path.file_name() {
                        if file_name.to_string_lossy().starts_with(&prefix) {
                            fs::remove_file(path).unwrap();
                        }
                    }
                }
            }

            cmd.arg("--lib");
        }
        CompileKind::Bin => {
            cmd.args(["--bin", &target.name]);
        }
        CompileKind::ProcMacro => {}
    }

    // 设置环境变量，指定特定的 crate 和目标
    cmd.env(SPECIFIC_CRATE, pkg.name.replace('-', "_"));
    cmd.env(SPECIFIC_TARGET, kind_str);

    tracing::debug!(
        "Package: {}, target kind {}, target name {}",
        pkg.name,
        kind_str,
        target.name
    );
}
