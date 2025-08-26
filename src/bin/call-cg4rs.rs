use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tokio::signal;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // 设置信号处理
    let child_processes = Arc::new(Mutex::new(HashSet::<u32>::new()));
    setup_signal_handling(child_processes.clone()).await;

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
    println!("manifest_path: {:?}", manifest_path);

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
        if let Some(path) = &manifest_path {
            clean_args.push(format!("--manifest-path={}", path.display()));
        }

        println!("Executing: cargo {}", clean_args.join(" "));

        // 一次性传递所有参数，使用 tokio 异步执行
        let mut child = create_async_process_group_command("cargo", &child_processes)
            .args(clean_args)
            .spawn()
            .expect("Failed to execute cargo clean");

        // 记录子进程 PID
        if let Some(pid) = child.id() {
            child_processes.lock().await.insert(pid);
        }

        let status = child.wait().await.expect("Failed to wait for cargo clean");

        // 清理 PID 记录
        if let Some(pid) = child.id() {
            child_processes.lock().await.remove(&pid);
        }

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

    // 一次性传递所有参数，使用 tokio 异步执行
    let mut child = create_async_process_group_command("cargo", &child_processes)
        .args(cg_args)
        .spawn()
        .expect("Failed to execute cargo cg4rs");

    // 记录子进程 PID
    if let Some(pid) = child.id() {
        child_processes.lock().await.insert(pid);
    }

    let status = child.wait().await.expect("Failed to wait for cargo cg4rs");

    // 清理 PID 记录
    if let Some(pid) = child.id() {
        child_processes.lock().await.remove(&pid);
    }

    if !status.success() {
        eprintln!("cargo cg4rs failed");
    }

    tracing::debug!("Finish to exec: cargo cg4rs");
    Ok(())
}

/// 设置信号处理，确保能优雅终止子进程
async fn setup_signal_handling(child_processes: Arc<Mutex<HashSet<u32>>>) {
    let child_processes_clone = child_processes.clone();

    tokio::spawn(async move {
        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
            .expect("Failed to install SIGINT handler");
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler");

        tokio::select! {
            _ = sigint.recv() => {
                eprintln!("Received SIGINT, terminating child processes...");
                terminate_child_processes(&child_processes_clone).await;
                std::process::exit(130); // 128 + SIGINT(2)
            }
            _ = sigterm.recv() => {
                eprintln!("Received SIGTERM, terminating child processes...");
                terminate_child_processes(&child_processes_clone).await;
                std::process::exit(143); // 128 + SIGTERM(15)
            }
        }
    });
}

/// 终止所有子进程
async fn terminate_child_processes(child_processes: &Arc<Mutex<HashSet<u32>>>) {
    let pids = {
        let guard = child_processes.lock().await;
        guard.clone()
    };

    for pid in pids {
        // 首先尝试优雅终止
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    // 等待一小段时间让进程优雅退出
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // 强制终止仍在运行的进程
    let pids = {
        let guard = child_processes.lock().await;
        guard.clone()
    };

    for pid in pids {
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

/// 创建一个新的进程组命令，便于管理子进程
fn create_async_process_group_command(
    program: &str,
    _child_processes: &Arc<Mutex<HashSet<u32>>>,
) -> Command {
    let mut cmd = Command::new(program);

    // 在 Unix 系统上创建新的进程组
    #[cfg(unix)]
    {
        // 创建新的进程组
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
    }

    cmd
}
