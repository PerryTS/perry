use super::*;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::commands::stdlib_features::{compute_required_features, features_to_cargo_arg};
use crate::OutputFormat;

use super::super::library_search::{find_harmonyos_sdk, harmonyos_cross_env};
use super::super::{find_perry_workspace_root, rust_target_triple, CompilationContext};

/// (#1529) Android's `libperry_app.so` is loaded via `dlopen`, so its TLS
/// relocations must use the global-dynamic model — the aarch64-linux-android
/// default (Initial-Executable) crashes at load with
/// `TLS symbol "(null)" ... using IE access model`. The model is selected by a
/// `tls-model` rustc flag, but that flag is exposed as a stable `-C` codegen
/// option on some toolchains and is still nightly-gated (`-Z`) on others.
/// Passing the `-C` form to a toolchain that only knows the `-Z` form aborts
/// *every* Android build with `error: unknown codegen option: tls-model`.
/// (This slipped past CI because release CI builds the runtime libs with plain
/// `cargo build` and never compiles a full Android app through this path.)
///
/// Probe the active rustc and return the spelling it accepts. When only the
/// `-Z` form is available, also set `RUSTC_BOOTSTRAP=1` on `cmd` so the gated
/// flag is honored on a stable toolchain without requiring a nightly install.
pub(crate) fn android_global_dynamic_tls_rustflag(cmd: &mut Command) -> &'static str {
    let c_form_supported = Command::new("rustc")
        .args(["-C", "help"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("tls-model"))
        .unwrap_or(false);
    if c_form_supported {
        "-C tls-model=global-dynamic"
    } else {
        cmd.env("RUSTC_BOOTSTRAP", "1");
        "-Z tls-model=global-dynamic"
    }
}

#[cfg(windows)]
pub(crate) fn cargo_target_dir_path(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", rest))
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(not(windows))]
pub(crate) fn cargo_target_dir_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn cargo_target_dir_env_path(_target_dir: &Path, relative_target_dir: &Path) -> PathBuf {
    relative_target_dir.to_path_buf()
}

#[cfg(not(windows))]
fn cargo_target_dir_env_path(target_dir: &Path, _relative_target_dir: &Path) -> PathBuf {
    target_dir.to_path_buf()
}

pub(crate) fn auto_target_dir_paths(workspace_root: &Path, hash: u64) -> (PathBuf, PathBuf) {
    let workspace_root = cargo_target_dir_path(workspace_root.to_path_buf());
    let relative_target_dir = PathBuf::from("target").join(format!("perry-auto-{:016x}", hash));
    let target_dir = cargo_target_dir_path(workspace_root.join(&relative_target_dir));
    let cargo_env_dir = cargo_target_dir_env_path(&target_dir, &relative_target_dir);
    (target_dir, cargo_env_dir)
}
