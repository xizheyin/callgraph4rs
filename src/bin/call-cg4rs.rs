use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tokio::signal;
use tokio::sync::Mutex;

struct Args {
    skip_clean: bool,
    project_root_dir: PathBuf,
    manifest_path: Option<PathBuf>,
    args_without_no_clean: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let desendent_processes = Arc::new(Mutex::new(HashSet::<u32>::new()));
    // 启动进程树监控
    start_process_tree_monitoring(desendent_processes.clone()).await;
    // 设置信号处理，收到SIGINT/SIGTERM时，终止子孙进程
    setup_signal_handling(desendent_processes.clone()).await;

    std::env::set_var("RUSTFLAGS", "-Zalways-encode-mir --cap-lints allow");

    // 获取命令行参数
    let Args {
        skip_clean,
        project_root_dir,
        manifest_path,
        args_without_no_clean,
    } = args().await;

    copy_toolchain_file(&project_root_dir).await?;
    cargo_clean(skip_clean, manifest_path.as_deref()).await?;
    cargo_cg4rs(args_without_no_clean).await?;
    Ok(())
}

async fn args() -> Args {
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
    println!("manifest_path: {manifest_path:?}");

    // 确定根目录，优先级：1. root_path 2. manifest_path所在目录 3. 当前目录
    let project_root_dir = if let Some(path) = root_path {
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
        let manifest_path = project_root_dir.join("Cargo.toml");
        if manifest_path.exists() {
            final_args.push(format!("--manifest-path={}", manifest_path.display()));
        } else {
            eprintln!(
                "Warning: Cargo.toml not found at {}",
                manifest_path.display()
            );
        }
    }

    // 检查是否跳过clean
    let skip_clean = final_args.iter().any(|arg| arg == "--no-clean");

    // 过滤掉--no-clean参数
    let args_without_no_clean: Vec<String> = final_args
        .iter()
        .filter(|&arg| arg != "--no-clean")
        .cloned()
        .collect();

    Args {
        skip_clean,
        project_root_dir,
        manifest_path,
        args_without_no_clean,
    }
}

async fn copy_toolchain_file(project_root_dir: &Path) -> anyhow::Result<()> {
    // 获取用户的主目录
    let home_dir = env::var("HOME").expect("Could not find home directory");
    let cargo_toolchain_path = Path::new(&home_dir).join(".cargo/rust-toolchain.toml");
    // Check if ~/.cargo/rust-toolchain.toml exists
    // This is because in build.rs, we copy the rust-toolchain.toml to `~/.cargo/rust-toolchain.toml`
    // We should copy it to the root directory of the project we are analyzing
    // to make sure the toolchain is the same as the one used to build the project
    if !cargo_toolchain_path.exists() {
        eprintln!("rust-toolchain.toml not found in ~/.cargo.");
        return Ok(());
    }
    // Copy toolchain file to the root directory of the project we are analyzing
    let target_path = project_root_dir.join("rust-toolchain.toml");
    fs::copy(&cargo_toolchain_path, &target_path)?;
    Ok(())
}

async fn cargo_clean(skip_clean: bool, manifest_path: Option<&Path>) -> anyhow::Result<()> {
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
        let mut child = Command::new("cargo")
            .args(clean_args)
            .spawn()
            .expect("Failed to execute cargo clean");

        let status = child.wait().await.expect("Failed to wait for cargo clean");

        if !status.success() {
            eprintln!("cargo clean failed");
            return Ok(());
        }
        tracing::trace!("Finish to cargo clean.");
    } else {
        tracing::debug!("Skip to clean.");
    }
    Ok(())
}

async fn cargo_cg4rs(args_without_no_clean: Vec<String>) -> anyhow::Result<()> {
    // 执行cargo cg4rs命令
    let mut cg_args = vec!["cg4rs".to_string()];
    cg_args.extend(args_without_no_clean.clone());

    println!("Executing: cargo {}", cg_args.join(" "));

    // 一次性传递所有参数，使用 tokio 异步执行
    let mut child = Command::new("cargo")
        .args(cg_args)
        .spawn()
        .expect("Failed to execute cargo cg4rs");

    let status = child.wait().await.expect("Failed to wait for cargo cg4rs");

    if !status.success() {
        eprintln!("cargo cg4rs failed");
    }

    tracing::debug!("Finish to exec: cargo cg4rs");
    Ok(())
}

/// 启动进程树监控，定期检查并记录所有子进程
pub async fn start_process_tree_monitoring(child_processes: Arc<Mutex<HashSet<u32>>>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            // 获取当前记录的直接子进程
            let direct_children = {
                let guard = child_processes.lock().await;
                guard.clone()
            };

            // 为每个直接子进程找到所有后代进程
            for &pid in &direct_children {
                if let Ok(descendants) = get_process_descendants(pid) {
                    // 将后代进程也加入监控列表
                    let mut guard = child_processes.lock().await;
                    for descendant in descendants {
                        guard.insert(descendant);
                    }
                }
            }
        }
    });
}

/// 获取指定进程的所有后代进程ID
pub fn get_process_descendants(pid: u32) -> Result<Vec<u32>, std::io::Error> {
    let mut descendants = Vec::new();
    let mut to_check = vec![pid];

    while let Some(current_pid) = to_check.pop() {
        // 读取 /proc 查找子进程
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                if let Ok(entry_name) = entry.file_name().into_string() {
                    if let Ok(entry_pid) = entry_name.parse::<u32>() {
                        // 读取 /proc/PID/stat 获取父进程ID
                        let stat_path = format!("/proc/{entry_pid}/stat");
                        if let Ok(stat_content) = std::fs::read_to_string(&stat_path) {
                            if let Some(ppid) = extract_ppid_from_stat(&stat_content) {
                                if ppid == current_pid {
                                    descendants.push(entry_pid);
                                    to_check.push(entry_pid); // 递归查找孙进程
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(descendants)
}

/// 从 /proc/PID/stat 内容中提取父进程ID
fn extract_ppid_from_stat(stat_content: &str) -> Option<u32> {
    let fields: Vec<&str> = stat_content.split_whitespace().collect();
    if fields.len() >= 4 {
        fields[3].parse().ok()
    } else {
        None
    }
}

/// 设置信号处理，确保能优雅终止子孙进程
pub async fn setup_signal_handling(child_processes: Arc<Mutex<HashSet<u32>>>) {
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

    eprintln!("Terminating {} processes...", pids.len());

    for pid in &pids {
        // 首先尝试优雅终止单个进程
        unsafe {
            let result = libc::kill(*pid as libc::pid_t, libc::SIGTERM);
            if result == 0 {
                eprintln!("Sent SIGTERM to process {pid}");
            }
        }
    }

    // 等待一小段时间让进程优雅退出
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // 强制终止仍在运行的进程
    for pid in &pids {
        unsafe {
            let result = libc::kill(*pid as libc::pid_t, libc::SIGKILL);
            if result == 0 {
                eprintln!("Sent SIGKILL to process {pid}");
            }
        }
    }
}
