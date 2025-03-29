use std::env;
use std::fs;
use std::path::Path;
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

    // 获取当前工作目录
    let current_dir = env::current_dir()?;
    let target_path = current_dir.join("rust-toolchain.toml");

    // 复制文件到当前 crate 的根目录
    fs::copy(&cargo_toolchain_path, &target_path)?;

    // 获取命令行参数并传递给 `cargo cg`
    let args: Vec<String> = env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--no-clean") {
        tracing::debug!("Skip to clean.");
    } else {
        tracing::trace!("Start to cargo clean.");
        // 执行 `cargo clean`
        let clean_status = Command::new("cargo")
            .arg("clean")
            .status()
            .expect("Failed to execute cargo clean");

        if !clean_status.success() {
            eprintln!("cargo clean failed");
            return Ok(());
        }
        tracing::trace!("Finish to cargo clean.");
    }

    let mut binding = Command::new("cargo");
    let cmd = binding.arg("cg").args(&args);
    tracing::debug!("Start to exec: {:?}", cmd);
    let status = cmd.status().expect("Failed to execute cargo cg");

    if !status.success() {
        eprintln!("cargo cg failed");
    }
    tracing::debug!("Finish to exec: {:?}", cmd);
    Ok(())
}
