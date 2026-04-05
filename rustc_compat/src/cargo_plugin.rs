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

// Environment variable constants used to control plugin behavior
pub const RUN_ON_ALL_CRATES: &str = "RUSTC_PLUGIN_ALL_TARGETS";
pub const SPECIFIC_CRATE: &str = "SPECIFIC_CRATE";
pub const SPECIFIC_TARGET: &str = "SPECIFIC_TARGET";
pub const CARGO_VERBOSE: &str = "CARGO_VERBOSE";

/// Main entry point for the cargo-side CLI tool
pub fn cargo_main<T: Plugin>(plugin: T) {
    // Check whether the version flag `-V` was passed
    if env::args().any(|arg| arg == "-V") {
        println!("{}\nversion={}", plugin.driver_name(), plugin.version());
        return;
    }

    // Build and execute the metadata command
    tracing::trace!("Fetch metadata");
    let metadata = build_metadata_command().exec().unwrap();

    // Set the plugin target directory
    let plugin_subdir = format!("plugin-{}", option_env!("RUSTC_CHANNEL").unwrap_or("default"));
    let target_dir = metadata.target_directory.join(plugin_subdir);

    // Collect plugin arguments
    let args = plugin.args(&target_dir);

    // Create the `cargo` command
    let mut cmd = Command::new("cargo");
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    // Resolve the current executable path
    // i.e. dir_path/cg4rs
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

    // Configure environment variables and command arguments
    cmd.env("RUSTC_WORKSPACE_WRAPPER", path)
        .arg("check")
        .arg("--target-dir")
        .arg(&target_dir);

    // Add --manifest-path when provided
    if let Some(manifest_path) = find_manifest_path() {
        cmd.arg("--manifest-path").arg(manifest_path);
    }

    // Configure cargo verbosity from the environment
    if env::var(CARGO_VERBOSE).is_ok() {
        cmd.arg("-vv");
    } else {
        cmd.arg("-q");
    }

    // Collect workspace members
    let workspace_members = metadata
        .workspace_members
        .iter()
        .map(|pkg_id| metadata.index(pkg_id))
        .collect::<Vec<_>>();

    // Decide how to run the plugin based on the filter type
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

    // Serialize plugin arguments to JSON and pass them via the environment
    let args_str = serde_json::to_string(&args.plugin_args).unwrap();
    tracing::debug!("{PLUGIN_ARGS}={args_str}");
    cmd.env(PLUGIN_ARGS, args_str);

    // Special handling for rustc workspace builds
    if workspace_members
        .iter()
        .any(|pkg| pkg.name == PackageName::new("rustc-main".to_string()).unwrap())
    {
        cmd.env("CFG_RELEASE", "");
    }

    // Allow the plugin to modify the cargo command
    plugin.modify_cargo(&mut cmd, &args.cargo_args);

    tracing::info!("Start to Exec: {:?}", cmd);
    // Execute the cargo command and exit with its status
    let exit_status = cmd.status().expect("failed to wait for cargo?");
    tracing::info!("Finish to Exec {:?}", cmd);
    exit(exit_status.code().unwrap_or(-1));
}

/// Build the metadata command
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

/// Find --manifest-path in command-line arguments
fn find_manifest_path() -> Option<String> {
    let args: Vec<String> = env::args().skip(1).collect();

    for i in 0..args.len() {
        if args[i].starts_with("--manifest-path=") {
            // Form: --manifest-path=/path/to/Cargo.toml
            let parts: Vec<&str> = args[i].splitn(2, '=').collect();
            if parts.len() == 2 {
                return Some(parts[1].to_string());
            }
        } else if args[i] == "--manifest-path" && i + 1 < args.len() {
            // Form: --manifest-path /path/to/Cargo.toml
            return Some(args[i + 1].clone());
        }
    }

    None
}

/// Run the plugin only for the crate containing the target file
fn only_run_on_file(
    cmd: &mut Command,
    file_path: PathBuf,
    workspace_members: &[&cargo_metadata::Package],
    target_dir: &Utf8Path,
) {
    // Normalize the file path for consistent matching
    let file_path = file_path.canonicalize().unwrap();

    // Find matching packages and targets for the file path
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
                    // When multiple targets match, try matching by file stem
                    let stem = file_path.file_stem().unwrap().to_string_lossy();
                    let name_matches_stem = targets.clone().into_iter().find(|target| target.name == stem);

                    // Special handling for main.rs binary targets
                    name_matches_stem.or_else(|| {
                        let only_bin = targets.iter().all(|target| !target.kind.contains(&"lib".into()));
                        if only_bin {
                            targets.into_iter().find(|target| target.kind.contains(&"bin".into()))
                        } else {
                            let kind = (if stem == "main" { "bin" } else { "lib" }).to_string();
                            targets
                                .into_iter()
                                .find(|target| target.kind.contains(&cargo_metadata::TargetKind::from(kind.as_str())))
                        }
                    })
                }
            })?;

            Some((pkg, target))
        })
        .collect::<Vec<_>>();

    // Ensure that exactly one matching target is found
    let (pkg, target) = match matching.len() {
        0 => panic!("Could not find target for path: {}", file_path.display()),
        1 => matching.remove(0),
        _ => panic!("Too many matching targets: {matching:?}"),
    };

    // Set the compilation filter for the selected target
    cmd.arg("-p").arg(format!("{}:{}", pkg.name, pkg.version));

    enum CompileKind {
        Lib,
        Bin,
        ProcMacro,
    }

    // Set compilation options based on the target kind
    let kind_str = &target.kind[0].to_string();
    let kind = match kind_str.as_str() {
        "lib" | "rlib" | "dylib" | "staticlib" | "cdylib" => CompileKind::Lib,
        "bin" => CompileKind::Bin,
        "proc-macro" => CompileKind::ProcMacro,
        _ => unreachable!("unexpected cargo crate type: {kind_str}"),
    };

    match kind {
        CompileKind::Lib => {
            // Remove previously generated library metadata files to avoid cache issues
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

    // Set environment variables for the selected crate and target
    cmd.env(SPECIFIC_CRATE, pkg.name.replace('-', "_"));
    cmd.env(SPECIFIC_TARGET, kind_str);

    tracing::debug!(
        "Package: {}, target kind {}, target name {}",
        pkg.name,
        kind_str,
        target.name
    );
}
