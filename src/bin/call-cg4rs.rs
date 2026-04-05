use std::env;
use std::path::{Path, PathBuf};
use tokio::fs as tokio_fs;
use tokio::process::Command;
use toml::Value as TomlValue;

struct Args {
    skip_clean: bool,
    project_root_dir: PathBuf,
    manifest_path: Option<PathBuf>,
    args: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Args {
        skip_clean,
        project_root_dir,
        manifest_path,
        args,
    } = args().await;

    cargo_clean(skip_clean, &project_root_dir, manifest_path.as_deref()).await?;

    cargo_cg4rs(args).await?;
    Ok(())
}

fn parse_path_flag(args: &[String], index: usize, flag: &str) -> Option<(PathBuf, usize)> {
    let current = args.get(index)?;
    if current == flag {
        let value = args.get(index + 1)?;
        return Some((PathBuf::from(value), 1));
    }

    let prefix = format!("{flag}=");
    current.strip_prefix(&prefix).map(|value| (PathBuf::from(value), 0))
}

async fn args() -> Args {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut root_path = None;
    let mut manifest_path = None;
    let mut filtered_args = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if let Some((path, consumed_next)) = parse_path_flag(&args, i, "--root-path") {
            root_path = Some(path);
            i += consumed_next;
        } else if let Some((path, consumed_next)) = parse_path_flag(&args, i, "--manifest-path") {
            manifest_path = Some(path.clone());
            filtered_args.push(format!("--manifest-path={}", path.display()));
            i += consumed_next;
        } else {
            filtered_args.push(args[i].clone());
        }
        i += 1;
    }

    // ensure target project root directory
    // 1. root_path
    // 2. Directory name of manifest_path (use manifest_path when using tools)
    // 3. current directory
    let project_root_dir = if let Some(path) = root_path {
        path
    } else if let Some(path) = &manifest_path {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"))
    } else {
        eprintln!("Warning: Neither --root-path nor --manifest-path provided. Using current directory.");
        env::current_dir().expect("Failed to get current directory")
    };

    let mut final_args = filtered_args;
    if manifest_path.is_none() {
        let inferred_manifest_path = project_root_dir.join("Cargo.toml");
        if inferred_manifest_path.exists() {
            manifest_path = Some(inferred_manifest_path.clone());
            final_args.push(format!("--manifest-path={}", inferred_manifest_path.display()));
        } else {
            eprintln!("Warning: Cargo.toml not found at {}", inferred_manifest_path.display());
        }
    }

    let skip_clean = final_args.iter().any(|arg| arg == "--no-clean");

    let args: Vec<String> = final_args.iter().filter(|&arg| arg != "--no-clean").cloned().collect();

    Args {
        skip_clean,
        project_root_dir,
        manifest_path,
        args,
    }
}

fn toolchain_channel_from_embedded() -> Option<String> {
    let content = include_str!("../../rust-toolchain.toml");
    let parsed: TomlValue = toml::from_str(content).ok()?;
    match &parsed {
        TomlValue::Table(t) => {
            // support both legacy and modern layout
            if let Some(TomlValue::Table(tl)) = t.get("toolchain") {
                if let Some(TomlValue::String(ch)) = tl.get("channel") {
                    return Some(ch.clone());
                }
            }
            if let Some(TomlValue::String(ch)) = t.get("channel") {
                return Some(ch.clone());
            }
            None
        }
        _ => None,
    }
}

async fn cargo_clean(skip_clean: bool, project_root_dir: &Path, manifest_path: Option<&Path>) -> anyhow::Result<()> {
    if skip_clean {
        tracing::debug!("Skip to clean.");
        return Ok(());
    }

    tracing::trace!("Start to cargo clean.");

    let target_dir = project_root_dir.join("target");

    if !target_dir.exists() {
        return Ok(());
    }

    // use rm
    tracing::debug!("Delete target dir directly: {}", target_dir.display());
    match tokio_fs::remove_dir_all(&target_dir).await {
        Ok(_) => return Ok(()),
        Err(e) => {
            tracing::warn!(
                "Failed to delete target dir {}, fallback to cargo clean: {}",
                target_dir.display(),
                e
            );
        }
    }

    let mut clean_args: Vec<String> = Vec::new();
    if let Some(tc) = toolchain_channel_from_embedded() {
        clean_args.push(format!("+{}", tc));
    }
    clean_args.push("clean".to_string());

    if let Some(path) = &manifest_path {
        clean_args.push(format!("--manifest-path={}", path.display()));
    }

    println!("Executing: cargo {}", clean_args.join(" "));

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
    Ok(())
}

async fn cargo_cg4rs(args: Vec<String>) -> anyhow::Result<()> {
    let mut cg_args: Vec<String> = Vec::new();
    if let Some(tc) = toolchain_channel_from_embedded() {
        cg_args.push(format!("+{}", tc));
    }
    cg_args.push("cg4rs".to_string());
    cg_args.extend(args.clone());

    println!("Executing: cargo {}", cg_args.join(" "));

    unsafe {
        std::env::set_var("RUSTFLAGS", "-Zalways-encode-mir --cap-lints allow");
    }
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
