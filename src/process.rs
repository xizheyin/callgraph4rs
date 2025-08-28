use std::collections::HashSet;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::Mutex;

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
