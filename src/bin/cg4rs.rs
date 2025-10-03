#![feature(rustc_private)]

use cg4rs::CGDriver;
use rustc_compat::rustc_main;
use std::process;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    tracing::trace!("run cg4rs");
    
    // 设置 5 分钟超时
    let timeout_duration = Duration::from_secs(5 * 60); // 5 分钟
    
    let result = timeout(timeout_duration, async {
        // 在异步任务中运行 rustc_main
        tokio::task::spawn_blocking(|| {
            rustc_main(CGDriver);
        }).await
    }).await;
    
    match result {
        Ok(Ok(())) => {
            tracing::info!("cg4rs completed successfully");
        }
        Ok(Err(e)) => {
            tracing::error!("cg4rs task failed: {:?}", e);
            process::exit(1);
        }
        Err(_) => {
            tracing::error!("cg4rs timed out after 5 minutes, terminating...");
            eprintln!("Error: cg4rs execution timed out after 5 minutes");
            process::exit(124); // 使用 124 作为超时退出码（类似 timeout 命令）
        }
    }
}
