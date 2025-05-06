use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> std::io::Result<()> {
    // 完全禁用日志
    //std::env::set_var("RUST_LOG", "debug");
    std::env::set_var("RUSTFLAGS", "-Zalways-encode-mir --cap-lints allow");

    // 获取用户的主目录
    let home_dir = env::var("HOME").expect("Could not find home directory");
    let cargo_toolchain_path = Path::new(&home_dir).join(".cargo/rust-toolchain.toml");

    // 检查 ~/.cargo/rust-toolchain.toml 是否存在
    if !cargo_toolchain_path.exists() {
        eprintln!("rust-toolchain.toml not found in ~/.cargo.");
        return Ok(());
    }

    // 获取命令行参数
    let args: Vec<String> = env::args().skip(1).collect();

    // 处理root-path参数
    let mut root_path = None;
    let mut manifest_path = None;
    let mut filtered_args = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i].starts_with("--root-path=") {
            // 形式: --root-path=/path/to/repo
            let path_str = args[i].split('=').nth(1).unwrap_or("");
            root_path = Some(PathBuf::from(path_str));
            // 不将此参数传递给cargo cg4rs
        } else if args[i] == "--root-path" && i + 1 < args.len() {
            // 形式: --root-path /path/to/repo
            root_path = Some(PathBuf::from(&args[i + 1]));
            i += 1; // 跳过下一个参数
                    // 不将此参数传递给cargo cg4rs
        } else if args[i].starts_with("--manifest-path=") {
            // 形式: --manifest-path=/path/to/repo/Cargo.toml
            let path_str = args[i].split('=').nth(1).unwrap_or("");
            manifest_path = Some(PathBuf::from(path_str));
            filtered_args.push(args[i].clone());
        } else if args[i] == "--manifest-path" && i + 1 < args.len() {
            // 形式: --manifest-path /path/to/repo/Cargo.toml
            manifest_path = Some(PathBuf::from(&args[i + 1]));
            filtered_args.push(args[i].clone());
            filtered_args.push(args[i + 1].clone());
            i += 1; // 跳过下一个参数
        } else {
            filtered_args.push(args[i].clone());
        }
        i += 1;
    }

    // 确定根目录，优先级：1. root_path 2. manifest_path所在目录 3. 当前目录
    let root_dir = if let Some(path) = root_path {
        path
    } else if let Some(path) = &manifest_path {
        // 如果提供了manifest路径，则使用其父目录
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"))
    } else {
        env::current_dir().expect("Failed to get current directory")
    };

    // 构造manifest-path（除非已经在参数中指定）
    let has_manifest_path = manifest_path.is_some();

    let mut final_args = filtered_args.clone();
    if !has_manifest_path {
        let manifest_path = root_dir.join("Cargo.toml");
        if manifest_path.exists() {
            final_args.push(format!("--manifest-path={}", manifest_path.display()));
        } else {
            eprintln!(
                "Warning: Cargo.toml not found at {}",
                manifest_path.display()
            );
        }
    }

    // 复制toolchain文件到根目录
    let target_path = root_dir.join("rust-toolchain.toml");
    fs::copy(&cargo_toolchain_path, &target_path)?;

    // 检查是否跳过clean
    let skip_clean = final_args.iter().any(|arg| arg == "--no-clean");

    // 过滤掉--no-clean参数
    let args_without_no_clean: Vec<String> = final_args
        .iter()
        .filter(|&arg| arg != "--no-clean")
        .cloned()
        .collect();

    if !skip_clean {
        tracing::trace!("Start to cargo clean.");

        // 收集clean命令的参数
        let mut clean_args = vec!["clean".to_string()];

        // 添加manifest-path参数
        for arg in &args_without_no_clean {
            if arg.starts_with("--manifest-path") {
                clean_args.push(arg.clone());
            }
        }

        println!("Executing: cargo {}", clean_args.join(" "));

        // 一次性传递所有参数
        let status = Command::new("cargo")
            .args(clean_args)
            .status()
            .expect("Failed to execute cargo clean");

        if !status.success() {
            eprintln!("cargo clean failed");
            return Ok(());
        }
        tracing::trace!("Finish to cargo clean.");
    } else {
        tracing::debug!("Skip to clean.");
    }

    // 执行cargo cg4rs命令
    let mut cg_args = vec!["cg4rs".to_string()];
    cg_args.extend(args_without_no_clean.clone());

    println!("Executing: cargo {}", cg_args.join(" "));

    // 一次性传递所有参数
    let status = Command::new("cargo")
        .args(cg_args)
        .status()
        .expect("Failed to execute cargo cg4rs");

    if !status.success() {
        eprintln!("cargo cg4rs failed");
    }

    tracing::debug!("Finish to exec: cargo cg4rs");
    Ok(())
}
