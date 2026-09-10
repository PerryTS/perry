//! Build and select Geisterhand's libraries as one Cargo graph (#10019).
//!
//! Two runtime crate instances can share a `perry_thread_local!` slot while
//! disagreeing on its Rust value layout. Archive existence is not evidence of
//! ABI compatibility: always let Cargo refresh the full set, then use only the
//! artifacts it reports (including on a fresh/cache-hit build).

use anyhow::{anyhow, bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{find_perry_workspace_root, is_android_target, is_windows_target, rust_target_triple};
use crate::OutputFormat;

#[derive(Debug)]
pub(crate) struct GeisterhandLibs {
    pub runtime: PathBuf,
    pub stdlib: PathBuf,
    pub ui: PathBuf,
    pub server: PathBuf,
}

fn build_command(workspace: &Path, ui_crate: &str, target: Option<&str>) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(workspace)
        .env("CARGO_TARGET_DIR", workspace.join("target/geisterhand"))
        .args([
            "build",
            "--release",
            "--message-format=json-render-diagnostics",
        ])
        .args([
            "-p",
            "perry-runtime-static",
            "--features",
            "perry-runtime/geisterhand",
        ])
        .args(["-p", "perry-stdlib-static"])
        .args([
            "-p",
            ui_crate,
            "--features",
            &format!("{ui_crate}/geisterhand"),
        ])
        .args(["-p", "perry-ui-geisterhand"]);
    if let Some(triple) = rust_target_triple(target) {
        cmd.args(["--target", triple]);
    }
    // The runtime also lands in Android's dlopen'd libperry_app.so (#1529).
    if is_android_target(target) {
        let flags = super::optimized_libs::android_global_dynamic_tls_rustflag(&mut cmd);
        cmd.env("RUSTFLAGS", flags);
    }
    cmd
}

fn artifacts_from_messages(messages: &[u8], ui_crate: &str) -> Result<GeisterhandLibs> {
    let ui_name = ui_crate.replace('-', "_");
    let names = [
        "perry_runtime",
        "perry_stdlib",
        &ui_name,
        "perry_ui_geisterhand",
    ];
    let mut artifacts: [Option<PathBuf>; 4] = Default::default();
    for line in messages.split(|b| *b == b'\n') {
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let target = &message["target"];
        let is_staticlib = target["crate_types"]
            .as_array()
            .is_some_and(|types| types.iter().any(|t| t == "staticlib"));
        if !is_staticlib {
            continue;
        }
        let Some(index) = names.iter().position(|name| target["name"] == *name) else {
            continue;
        };
        if let Some(files) = message["filenames"].as_array() {
            for filename in files.iter().filter_map(|f| f.as_str()) {
                let path = PathBuf::from(filename);
                if !matches!(path.extension().and_then(|e| e.to_str()), Some("a" | "lib")) {
                    continue;
                }
                if artifacts[index]
                    .as_ref()
                    .is_some_and(|previous| previous != &path)
                {
                    bail!(
                        "Cargo reported multiple Geisterhand archives for {}",
                        names[index]
                    );
                }
                artifacts[index] = Some(path);
            }
        }
    }
    for (name, artifact) in names.iter().zip(&artifacts) {
        let path = artifact.as_ref().ok_or_else(|| {
            anyhow!("Cargo did not report the Geisterhand {name} static library; refusing to link cached libraries from another build")
        })?;
        if !path.is_file() {
            bail!(
                "Cargo reported a missing Geisterhand library: {}",
                path.display()
            );
        }
    }
    let [runtime, stdlib, ui, server] = artifacts.map(Option::unwrap);
    Ok(GeisterhandLibs {
        runtime,
        stdlib,
        ui,
        server,
    })
}

fn run_build(cmd: &mut Command, ui_crate: &str, verbose: u8) -> Result<GeisterhandLibs> {
    if verbose > 0 {
        cmd.stderr(Stdio::inherit());
    }
    let output = cmd
        .output()
        .context("Failed to run cargo for Geisterhand")?;
    // json-render-diagnostics keeps human-readable diagnostics on stderr.
    if !output.status.success() {
        let _ = std::io::stderr().write_all(&output.stderr);
    }
    if !output.status.success() {
        bail!(
            "Failed to build Geisterhand libraries (cargo exited with {})",
            output.status
        );
    }
    artifacts_from_messages(&output.stdout, ui_crate)
}

pub(super) fn build_geisterhand_libs(
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> Result<GeisterhandLibs> {
    if matches!(target, Some("visionos" | "visionos-simulator")) {
        bail!("Geisterhand is not supported on visionOS yet.");
    }
    let ui_crate = match target {
        Some("ios-simulator" | "ios") => "perry-ui-ios",
        target if is_android_target(target) => "perry-ui-android",
        Some("linux") => "perry-ui-gtk4",
        Some("windows-winui") => "perry-ui-windows-winui",
        target if is_windows_target(target) => "perry-ui-windows",
        _ if cfg!(target_os = "linux") => "perry-ui-gtk4",
        _ if cfg!(target_os = "windows") => "perry-ui-windows",
        _ => "perry-ui-macos",
    };
    let workspace = find_perry_workspace_root().ok_or_else(|| {
        anyhow!("Cannot build Geisterhand libraries: Perry workspace not found. Set PERRY_WORKSPACE_ROOT to a Perry source checkout so all libraries can be built together.")
    })?;
    if matches!(format, OutputFormat::Text) {
        println!("Preparing Geisterhand libraries ({ui_crate})...");
    }
    run_build(
        &mut build_command(&workspace, ui_crate, target),
        ui_crate,
        verbose,
    )
}

#[cfg(test)]
mod tests;
