//! Library + LLVM-toolchain search helpers.
//!
//! Extracted from `compile.rs` (Tier 2.1 of the compiler-improvement
//! plan, v0.5.333). This module bundles three closely-related concerns
//! that the link command construction depends on:
//!
//! - **LLVM toolchain locator** — `find_llvm_tool` (with rustup-sysroot,
//!   PATH, and PERRY_<TOOL> env-var overrides), MSVC `link.exe` /
//!   `lld-link` lookup, Windows SDK probing.
//! - **Static library locator** — `find_library_with_candidates` /
//!   `find_library` / `collect_library_candidates`, plus the per-lib
//!   wrappers (`find_runtime_library`, `find_stdlib_library`,
//!   `find_jsruntime_library`, `find_ui_library`).
//! - **Geisterhand integration** — the optional native-bridge crate
//!   that `find_geisterhand_*` searches for, plus its build helper.
//!
//! Most callers are inside `compile.rs` itself (link command
//! construction); a handful escape via re-export to the parent module.
//! `strip_dedup.rs` also uses `find_library`, `find_llvm_tool`, and
//! `find_stdlib_library` via `super::`.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OutputFormat;

// `rust_target_triple` and `find_perry_workspace_root` still live in
// the compile.rs orchestrator. Pull them in as private parent-module
// items so the search helpers below can reach them.
use super::{find_perry_workspace_root, rust_target_triple};

pub(super) fn find_llvm_tool(tool_name: &str) -> Option<PathBuf> {
    // 1. Env var override (e.g. PERRY_LLD_LINK for "lld-link")
    let env_key = format!("PERRY_{}", tool_name.to_uppercase().replace('-', "_"));
    if let Ok(path) = std::env::var(&env_key) {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Rust sysroot: lib/rustlib/<host-triple>/bin/<tool>
    if let Ok(output) = Command::new("rustc").arg("--print").arg("sysroot").output() {
        let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !sysroot.is_empty() {
            if let Ok(vv) = Command::new("rustc").arg("-vV").output() {
                let vv_str = String::from_utf8_lossy(&vv.stdout);
                if let Some(host_line) = vv_str.lines().find(|l| l.starts_with("host:")) {
                    let host_triple = host_line.trim_start_matches("host:").trim();
                    let exe_suffix = if cfg!(target_os = "windows") { ".exe" } else { "" };
                    let tool_path = PathBuf::from(&sysroot)
                        .join("lib").join("rustlib").join(host_triple).join("bin")
                        .join(format!("{}{}", tool_name, exe_suffix));
                    if tool_path.exists() {
                        return Some(tool_path);
                    }
                }
            }
        }
    }

    // 3. PATH lookup
    let which_cmd = if cfg!(target_os = "windows") { "where" } else { "which" };
    if let Ok(output) = Command::new(which_cmd).arg(tool_name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path.lines().next().unwrap_or(&path)));
            }
        }
    }

    None
}

/// Find MSVC link.exe by searching Visual Studio installation directories.
/// On Windows, the PATH may contain a GNU `link` utility (e.g. from Git Bash/MSYS2)
/// which is not the MSVC linker. This function searches for the real MSVC link.exe.
#[cfg(target_os = "windows")]
pub(super) fn msvc_vswhere_installation_path_args() -> [&'static str; 8] {
    [
        "-products",
        "*",
        // Without the VC tools filter, `-latest` can select Management Studio.
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-latest",
        "-property",
        "installationPath",
        "-nologo",
    ]
}

#[cfg(target_os = "windows")]
pub(super) fn find_msvc_link_exe() -> Option<PathBuf> {
    // Try vswhere.exe first (most reliable)
    let vswhere_paths = [
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe"),
    ];
    for vswhere in &vswhere_paths {
        if vswhere.exists() {
            if let Ok(output) = Command::new(vswhere)
                .args(msvc_vswhere_installation_path_args())
                .output()
            {
                let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !install_path.is_empty() {
                    // Search for link.exe under VC/Tools/MSVC/*/bin/Hostx64/x64/
                    let msvc_dir = PathBuf::from(&install_path).join(r"VC\Tools\MSVC");
                    if let Ok(entries) = std::fs::read_dir(&msvc_dir) {
                        let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                        versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
                        for entry in versions {
                            let link = entry.path().join(r"bin\Hostx64\x64\link.exe");
                            if link.exists() {
                                return Some(link);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub(super) fn find_msvc_link_exe() -> Option<PathBuf> {
    find_llvm_tool("lld-link")
}

/// Find `lld-link.exe` — LLVM's drop-in replacement for MSVC `link.exe`. Ships
/// with `winget install LLVM.LLVM`. Enables the "lightweight Windows toolchain"
/// path: LLVM for codegen + linking, xwin'd sysroot for CRT + Windows SDK libs,
/// no Visual Studio required. See `perry setup windows`.
///
/// Available on all hosts (not just Windows native): cross-compile callers on
/// macOS/Linux targeting Windows also want to locate a bundled lld-link
/// before falling back to vswhere-based MSVC detection.
pub(super) fn find_lld_link() -> Option<PathBuf> {
    // Honor explicit override (shared with MSVC path).
    if let Ok(p) = std::env::var("PERRY_LLD_LINK") {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    // Standard LLVM installer location.
    let standalone = PathBuf::from(r"C:\Program Files\LLVM\bin\lld-link.exe");
    if standalone.exists() {
        return Some(standalone);
    }
    // PATH fallback.
    if let Ok(output) = Command::new("where").arg("lld-link").output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(first) = s.lines().next() {
                let p = PathBuf::from(first);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Location where `perry setup windows` writes the xwin'd Microsoft CRT +
/// Windows SDK. Returns `Some(root)` only when `<root>/crt/lib/x86_64` exists,
/// so callers can treat `Some` as "toolchain is complete and ready to link."
///
/// Default location is `%LOCALAPPDATA%\perry\windows-sdk` on Windows; can be
/// overridden via `PERRY_WINDOWS_SYSROOT` (same env var already used by the
/// cross-compile branch, so a single env var works for both hosts).
/// Available on all hosts so the `is_windows` target branch (which fires on
/// macOS/Linux cross-compiles too) can check for an xwin'd Windows SDK without
/// needing its own cfg gate.
pub(super) fn find_perry_windows_sdk() -> Option<PathBuf> {
    let explicit = std::env::var("PERRY_WINDOWS_SYSROOT")
        .ok()
        .map(PathBuf::from);
    let default = dirs::data_local_dir().map(|p| p.join("perry").join("windows-sdk"));
    for candidate in [explicit, default].into_iter().flatten() {
        // Sanity-check: xwin splat populates crt/lib/x86_64 (or crt/lib/x64 with
        // --preserve-ms-arch-notation). If neither exists, the directory isn't a
        // completed xwin output — skip it.
        if candidate.join("crt").join("lib").join("x86_64").exists()
            || candidate.join("crt").join("lib").join("x64").exists()
        {
            return Some(candidate);
        }
    }
    None
}

/// Returns the `/SUBSYSTEM:…` flag for MSVC `link.exe` / `lld-link`.
///
/// CLI programs must use `CONSOLE` (3) so the OS loader attaches stdin/stdout/stderr
/// before `main()` runs. GUI programs use `WINDOWS` (2) to suppress the console
/// window that would otherwise flash alongside the app window. Passing neither
/// flag lets the linker pick a default, which historically resolved to `WINDOWS`
/// for Perry builds and silently discarded all `console.log` output (issue #120).
pub(super) fn windows_pe_subsystem_flag(needs_ui: bool) -> &'static str {
    if needs_ui { "/SUBSYSTEM:WINDOWS" } else { "/SUBSYSTEM:CONSOLE" }
}

/// Given a sysroot directory populated by `xwin splat` (or a compatible layout),
/// return the lib search paths for MSVC / lld-link's LIB env var. Callers pass
/// the directory root (e.g. `%LOCALAPPDATA%\perry\windows-sdk`) and get back a
/// `Vec<String>` of absolute lib dirs: `<root>/crt/lib/x86_64`,
/// `<root>/sdk/lib/um/x86_64`, `<root>/sdk/lib/ucrt/x86_64`. Falls through to
/// `<root>/lib` and finally `<root>` itself if the structured layout isn't
/// present (e.g. a user pointed PERRY_WINDOWS_SYSROOT at a custom dir).
pub(super) fn xwin_sysroot_lib_paths(root: &Path) -> Vec<String> {
    let mut paths = Vec::new();

    // xwin default layout — also covers --preserve-ms-arch-notation (x64 suffix).
    for (crt_sub, um_sub, ucrt_sub) in &[
        ("crt/lib/x86_64", "sdk/lib/um/x86_64", "sdk/lib/ucrt/x86_64"),
        ("crt/lib/x64", "sdk/lib/um/x64", "sdk/lib/ucrt/x64"),
    ] {
        let crt = root.join(crt_sub);
        let um = root.join(um_sub);
        let ucrt = root.join(ucrt_sub);
        if crt.exists() || um.exists() || ucrt.exists() {
            if crt.exists() { paths.push(crt.to_string_lossy().to_string()); }
            if um.exists() { paths.push(um.to_string_lossy().to_string()); }
            if ucrt.exists() { paths.push(ucrt.to_string_lossy().to_string()); }
            return paths;
        }
    }

    let flat_lib = root.join("lib");
    if flat_lib.exists() {
        paths.push(flat_lib.to_string_lossy().to_string());
        return paths;
    }

    paths.push(root.to_string_lossy().to_string());
    paths
}

/// Find MSVC library search paths (MSVC CRT, Windows SDK um, Windows SDK ucrt).
/// Returns a semicolon-separated string suitable for the LIB environment variable.
///
/// On Windows, prefers `perry setup windows`'s xwin'd sysroot when present
/// (matches the "lightweight toolchain" opt-in mental model), then falls back
/// to vswhere-located Visual Studio install paths.
#[cfg(target_os = "windows")]
pub(super) fn find_msvc_lib_paths() -> Option<String> {
    // If the user ran `perry setup windows`, use that sysroot — they've
    // expressed intent to use the lightweight LLVM + xwin path even if MSVC
    // is also installed. Same precedence as find_msvc_link_exe_or_lld_link().
    if let Some(sysroot) = find_perry_windows_sdk() {
        let paths = xwin_sysroot_lib_paths(&sysroot);
        if !paths.is_empty() {
            return Some(paths.join(";"));
        }
    }

    let mut paths = Vec::new();

    // Find MSVC CRT lib path via vswhere
    let vswhere_paths = [
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"),
        PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\Installer\vswhere.exe"),
    ];
    for vswhere in &vswhere_paths {
        if vswhere.exists() {
            if let Ok(output) = Command::new(vswhere)
                .args(msvc_vswhere_installation_path_args())
                .output()
            {
                let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !install_path.is_empty() {
                    let msvc_dir = PathBuf::from(&install_path).join(r"VC\Tools\MSVC");
                    if let Ok(entries) = std::fs::read_dir(&msvc_dir) {
                        let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                        versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
                        if let Some(entry) = versions.first() {
                            let lib_path = entry.path().join(r"lib\x64");
                            if lib_path.exists() {
                                paths.push(lib_path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
            break;
        }
    }

    // Find Windows SDK lib paths
    let sdk_root = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\Lib");
    if let Ok(entries) = std::fs::read_dir(&sdk_root) {
        let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        if let Some(entry) = versions.first() {
            let um_path = entry.path().join(r"um\x64");
            let ucrt_path = entry.path().join(r"ucrt\x64");
            if um_path.exists() {
                paths.push(um_path.to_string_lossy().to_string());
            }
            if ucrt_path.exists() {
                paths.push(ucrt_path.to_string_lossy().to_string());
            }
        }
    }

    if paths.is_empty() {
        None
    } else {
        Some(paths.join(";"))
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn find_msvc_lib_paths() -> Option<String> {
    let sysroot = std::env::var("PERRY_WINDOWS_SYSROOT").ok()?;
    let root = PathBuf::from(&sysroot);
    if !root.exists() {
        eprintln!("Warning: PERRY_WINDOWS_SYSROOT={} does not exist", root.display());
        return None;
    }

    Some(xwin_sysroot_lib_paths(&root).join(";"))
}

/// Find a library by name, optionally searching cross-compilation target directories.
///
/// Returns the located path, or a list of all searched candidate paths so the
/// caller can surface them in an error message.
pub(super) fn find_library_with_candidates(name: &str, target: Option<&str>) -> Result<PathBuf, Vec<PathBuf>> {
    let candidates = collect_library_candidates(name, target);
    for path in &candidates {
        if path.exists() {
            return Ok(path.clone());
        }
    }
    Err(candidates)
}

pub(super) fn find_library(name: &str, target: Option<&str>) -> Option<PathBuf> {
    find_library_with_candidates(name, target).ok()
}

pub(super) fn collect_library_candidates(name: &str, target: Option<&str>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // Env-var overrides: users can point at an out-of-tree build dir (e.g. when
    // the perry binary is copied to /usr/local/bin but the source tree lives
    // elsewhere). Checked first so an explicit override always wins.
    for env_var in ["PERRY_RUNTIME_DIR", "PERRY_LIB_DIR"] {
        if let Ok(dir) = std::env::var(env_var) {
            if !dir.is_empty() {
                candidates.push(PathBuf::from(&dir).join(name));
            }
        }
    }

    // For cross-compilation targets, ONLY search target-specific directories
    // to avoid linking host-platform libraries into the wrong target
    if let Some(triple) = rust_target_triple(target) {
        candidates.push(PathBuf::from(format!("target/{}/release/{}", triple, name)));
        candidates.push(PathBuf::from(format!("target/{}/debug/{}", triple, name)));
        // When targeting the host platform (e.g. --target windows on Windows),
        // also check the default target/release/ directory since native builds
        // put libraries there without the triple subdirectory.
        #[cfg(target_os = "windows")]
        if matches!(target, Some("windows")) {
            candidates.push(PathBuf::from(format!("target/release/{}", name)));
            candidates.push(PathBuf::from(format!("target/debug/{}", name)));
        }
        #[cfg(target_os = "linux")]
        if matches!(target, Some("linux")) {
            candidates.push(PathBuf::from(format!("target/release/{}", name)));
            candidates.push(PathBuf::from(format!("target/debug/{}", name)));
        }
        // Also check directories relative to the perry executable.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                // Cross-compile targets are in ../../target/<triple>/release/ relative
                // to the perry binary (which is in target/release/). Check this
                // BEFORE the exe-dir bundled-install lookups below — in an
                // in-tree dev build, `target/release/libperry_ui_ios.a` is the
                // host-platform (macOS) artifact left over from a native build,
                // and would shadow the freshly cross-compiled iOS lib in
                // `target/aarch64-apple-ios-sim/release/`.
                if let Some(target_dir) = dir.parent() {
                    candidates.push(target_dir.join(triple).join("release").join(name));
                    candidates.push(target_dir.join(triple).join("debug").join(name));
                }
                // When cargo install'd, check the original source tree's target dir
                let source_target = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target").join(triple).join("release").join(name);
                candidates.push(source_target);

                // For iOS targets, check the exe directory for libs with _ios naming:
                // - Libs already named with _ios (e.g. libperry_ui_ios.a) → direct lookup
                // - Libs using _ios suffix convention (e.g. libperry_runtime.a stored as
                //   libperry_runtime_ios.a next to the binary)
                if matches!(target, Some("ios") | Some("ios-simulator") | Some("ios-widget") | Some("ios-widget-simulator")) {
                    if name.contains("_ios") {
                        candidates.push(dir.join(name));
                    } else {
                        let ios_name = name.replace(".a", "_ios.a");
                        candidates.push(dir.join(&ios_name));
                    }
                }
                if matches!(target, Some("visionos") | Some("visionos-simulator")) {
                    if name.contains("_visionos") {
                        candidates.push(dir.join(name));
                    } else {
                        let visionos_name = name.replace(".a", "_visionos.a");
                        candidates.push(dir.join(&visionos_name));
                    }
                }
                if matches!(target, Some("watchos") | Some("watchos-simulator")) {
                    if name.contains("_watchos") {
                        candidates.push(dir.join(name));
                    } else {
                        let watchos_name = name.replace(".a", "_watchos.a");
                        candidates.push(dir.join(&watchos_name));
                    }
                }
                if matches!(target, Some("tvos") | Some("tvos-simulator")) {
                    if name.contains("_tvos") {
                        candidates.push(dir.join(name));
                    } else {
                        let tvos_name = name.replace(".a", "_tvos.a");
                        candidates.push(dir.join(&tvos_name));
                    }
                }
                if matches!(target, Some("harmonyos") | Some("harmonyos-simulator")) {
                    if name.contains("_harmonyos") {
                        candidates.push(dir.join(name));
                    } else {
                        let harmonyos_name = name.replace(".a", "_harmonyos.a");
                        candidates.push(dir.join(&harmonyos_name));
                    }
                }
            }
        }
    } else {
        // Host build: search host directories
        candidates.push(PathBuf::from(format!("target/release/{}", name)));
        candidates.push(PathBuf::from(format!("target/debug/{}", name)));
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join(name));
                // Homebrew: libs installed in ../lib relative to bin
                if let Some(prefix) = dir.parent() {
                    candidates.push(prefix.join("lib").join(name));
                }
            }
        }
        // When cargo install'd, check the original source tree's target dir
        let source_target = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/release").join(name);
        candidates.push(source_target);
        candidates.push(PathBuf::from(format!("/usr/local/lib/{}", name)));
        // Debian/Ubuntu: libs installed in /usr/lib/perry
        candidates.push(PathBuf::from(format!("/usr/lib/perry/{}", name)));
    }

    candidates
}

/// Find the runtime library for linking
pub(super) fn find_runtime_library(target: Option<&str>) -> Result<PathBuf> {
    let lib_name = match target {
        Some("windows") => "perry_runtime.lib",
        #[cfg(target_os = "windows")]
        None => "perry_runtime.lib",
        _ => "libperry_runtime.a",
    };
    find_library_with_candidates(lib_name, target).map_err(|searched| {
        let extra = if target.is_some() {
            format!(" (for target {:?})", target.unwrap())
        } else {
            String::new()
        };
        let target_flag = rust_target_triple(target)
            .map(|t| format!(" --target {}", t))
            .unwrap_or_default();
        let searched_list = searched
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow!(
            "Could not find {lib}{extra}.\n\
             Searched:\n{list}\n\n\
             Fixes:\n\
             - From the perry workspace: cargo build --release -p perry-runtime{tf}\n\
             - Out-of-tree install: set PERRY_RUNTIME_DIR to the directory containing {lib}\n\
               (e.g. export PERRY_RUNTIME_DIR=/path/to/perry/target/release)",
            lib = lib_name,
            extra = extra,
            list = searched_list,
            tf = target_flag,
        )
    })
}

/// Find the stdlib library for linking (optional - only needed for native modules)
pub(super) fn find_stdlib_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("windows") => "perry_stdlib.lib",
        #[cfg(target_os = "windows")]
        None => "perry_stdlib.lib",
        _ => "libperry_stdlib.a",
    };
    find_library(lib_name, target)
}

/// Find the V8 jsruntime library for linking (optional - only needed for JS module support)
pub(super) fn find_jsruntime_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("windows") => "perry_jsruntime.lib",
        #[cfg(target_os = "windows")]
        None => "perry_jsruntime.lib",
        _ => "libperry_jsruntime.a",
    };
    find_library(lib_name, target)
}

/// Find the UI library for linking (optional - only needed when perry/ui is imported)
pub(super) fn find_ui_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("ios-simulator") | Some("ios") => "libperry_ui_ios.a",
        Some("visionos-simulator") | Some("visionos") => "libperry_ui_visionos.a",
        Some("android") => "libperry_ui_android.a",
        Some("watchos-simulator") | Some("watchos") => "libperry_ui_watchos.a",
        Some("tvos-simulator") | Some("tvos") => "libperry_ui_tvos.a",
        Some("harmonyos-simulator") | Some("harmonyos") => "libperry_ui_harmonyos.a",
        Some("linux") => "libperry_ui_gtk4.a",
        Some("macos") => "libperry_ui_macos.a",
        Some("windows") => "perry_ui_windows.lib",
        #[cfg(target_os = "windows")]
        None => "perry_ui_windows.lib",
        _ => {
            if cfg!(target_os = "linux") {
                "libperry_ui_gtk4.a"
            } else {
                "libperry_ui_macos.a"
            }
        }
    };
    find_library(lib_name, target)
}

/// Locate the OpenHarmony SDK's `native/` directory — the one that contains
/// `llvm/bin/clang` (the cross-compiler) and `sysroot/` (musl headers + libs).
///
/// Probes `$OHOS_SDK_HOME` first (user-supplied path; may point at either the
/// SDK root or the `native/` subdir — we normalize). Falls back to DevEco
/// Studio's default install locations per platform. Returns `None` if nothing
/// resembling an OHOS SDK is present; the caller is expected to surface a
/// remediation message naming the env var.
pub(super) fn find_harmonyos_sdk() -> Option<PathBuf> {
    fn normalize(p: PathBuf) -> Option<PathBuf> {
        // Accept either `<sdk>` or `<sdk>/native` — we want the `native` dir
        // so callers can unconditionally join `llvm/bin/clang` and `sysroot`.
        if p.join("llvm").join("bin").exists() && p.join("sysroot").exists() {
            return Some(p);
        }
        let native = p.join("native");
        if native.join("llvm").join("bin").exists() && native.join("sysroot").exists() {
            return Some(native);
        }
        // DevEco's layout nests the API-level dir: <root>/openharmony/<api>/native
        if let Ok(entries) = std::fs::read_dir(p.join("openharmony")) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("native");
                if candidate.join("llvm").join("bin").exists() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    if let Ok(env_path) = std::env::var("OHOS_SDK_HOME") {
        if let Some(sdk) = normalize(PathBuf::from(env_path)) {
            return Some(sdk);
        }
    }

    let home = std::env::var("HOME").ok().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(h) = home {
        // macOS default: DevEco Studio's "system image" + tooling SDK
        // installs into ~/Library/Huawei/Sdk — but the native cross-compiler
        // (clang + musl sysroot) actually lives inside the DevEco-Studio.app
        // bundle, not under the user's Library/Huawei dir. Probe the user
        // dir first in case someone unpacked a standalone OHOS SDK there,
        // then fall through to the bundle.
        candidates.push(h.join("Library/Huawei/Sdk"));
        // Linux default
        candidates.push(h.join("Huawei/Sdk"));
    }
    // macOS: DevEco Studio bundles the native cross-toolchain inside its
    // .app at `Contents/sdk/default/openharmony/native`. The "default"
    // segment is the active SDK profile selected in DevEco's prefs UI;
    // multi-profile installs may have other names alongside it (we'd
    // need to enumerate `Contents/sdk/*/openharmony/native` for those —
    // deferred until a user reports a non-default profile).
    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/DevEco-Studio.app/Contents/sdk/default/openharmony/native"
        ));
    }
    #[cfg(target_os = "windows")]
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        candidates.push(PathBuf::from(userprofile).join("Huawei").join("Sdk"));
    }

    for c in candidates {
        if let Some(sdk) = normalize(c) {
            return Some(sdk);
        }
    }
    None
}

/// Cross-compile env vars to pass to `cargo build` so `cc-rs` picks up the
/// OHOS SDK's clang + musl sysroot for any C source in dependency build.rs
/// scripts (notably `libmimalloc-sys`, which needs `pthread.h`).
///
/// Cargo reads both `CC_<triple>` and the underscored `CC_<TRIPLE>` form —
/// `cc-rs` prefers the latter. We set both for robustness. Same for linker.
pub(super) fn harmonyos_cross_env(sdk_native: &Path, target: Option<&str>) -> Vec<(String, String)> {
    let (triple, clang_target) = match target {
        Some("harmonyos-simulator") => ("x86_64-unknown-linux-ohos", "x86_64-linux-ohos"),
        _ => ("aarch64-unknown-linux-ohos", "aarch64-linux-ohos"),
    };
    let clang = sdk_native.join("llvm").join("bin").join("clang");
    let clangpp = sdk_native.join("llvm").join("bin").join("clang++");
    let sysroot = sdk_native.join("sysroot");
    let cflags = format!(
        "--target={} --sysroot={} -D__MUSL__",
        clang_target,
        sysroot.display()
    );
    let rustflags = format!(
        "-C link-arg=--target={} -C link-arg=--sysroot={}",
        clang_target,
        sysroot.display()
    );
    let triple_upper = triple.to_uppercase().replace('-', "_");
    let triple_under = triple.replace('-', "_");

    // CC + CXX: libmimalloc-sys compiles .c via CC and can fall into C++ paths
    // via CXX for some builds — we set both to the OHOS SDK toolchain so neither
    // escapes to the host `c++` (which lacks --sysroot and would fail with
    // "'pthread.h' file not found").
    vec![
        (format!("CC_{}", triple), clang.display().to_string()),
        (format!("CC_{}", triple_under), clang.display().to_string()),
        (format!("CXX_{}", triple), clangpp.display().to_string()),
        (format!("CXX_{}", triple_under), clangpp.display().to_string()),
        (format!("CFLAGS_{}", triple), cflags.clone()),
        (format!("CFLAGS_{}", triple_under), cflags.clone()),
        (format!("CXXFLAGS_{}", triple), cflags.clone()),
        (format!("CXXFLAGS_{}", triple_under), cflags),
        (format!("CARGO_TARGET_{}_LINKER", triple_upper), clang.display().to_string()),
        (format!("CARGO_TARGET_{}_RUSTFLAGS", triple_upper), rustflags),
    ]
}

/// Search for a geisterhand library by name, checking both cross-compilation
/// target dirs (target/geisterhand/{triple}/release/) and host dir (target/geisterhand/release/).
pub(super) fn find_geisterhand_lib(name: &str, target: Option<&str>) -> Option<PathBuf> {
    // Search relative to CWD first, then relative to the Perry workspace root.
    // Check both target/geisterhand/ (separate build dir) and target/ (shared build dir)
    // to support both build workflows.
    let search_roots: Vec<PathBuf> = {
        let mut roots = vec![PathBuf::from(".")];
        if let Some(ws) = find_perry_workspace_root() {
            roots.push(ws);
        }
        roots
    };
    for root in &search_roots {
        // Cross-compilation target dir first
        if let Some(triple) = rust_target_triple(target) {
            // Separate geisterhand build dir
            let path = root.join(format!("target/geisterhand/{}/release/{}", triple, name));
            if path.exists() { return Some(path); }
            // Shared release dir (when built with --features geisterhand in normal target)
            let path = root.join(format!("target/{}/release/{}", triple, name));
            if path.exists() { return Some(path); }
        }
        // Host build dir
        let path = root.join(format!("target/geisterhand/release/{}", name));
        if path.exists() { return Some(path); }
        let path = root.join(format!("target/release/{}", name));
        if path.exists() { return Some(path); }
    }
    None
}

pub(super) fn find_geisterhand_library(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("windows")) || cfg!(target_os = "windows") {
        "perry_ui_geisterhand.lib"
    } else {
        "libperry_ui_geisterhand.a"
    };
    find_geisterhand_lib(name, target)
        .or_else(|| find_library(name, None))
}

pub(super) fn find_geisterhand_runtime(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("windows")) || cfg!(target_os = "windows") {
        "perry_runtime.lib"
    } else {
        "libperry_runtime.a"
    };
    find_geisterhand_lib(name, target)
}

pub(super) fn find_geisterhand_ui(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("ios-simulator") | Some("ios")) {
        "libperry_ui_ios.a"
    } else if matches!(target, Some("visionos-simulator") | Some("visionos")) {
        return None;
    } else if matches!(target, Some("android")) {
        "libperry_ui_android.a"
    } else if matches!(target, Some("linux")) || cfg!(target_os = "linux") {
        "libperry_ui_gtk4.a"
    } else if matches!(target, Some("windows")) || cfg!(target_os = "windows") {
        "perry_ui_windows.lib"
    } else {
        "libperry_ui_macos.a"
    };
    find_geisterhand_lib(name, target)
}

/// Auto-build geisterhand-enabled libraries when they're missing.
/// Uses a separate target dir (target/geisterhand/) to avoid mixing with normal builds.
pub(super) fn build_geisterhand_libs(target: Option<&str>, format: OutputFormat) -> Result<()> {
    if matches!(target, Some("visionos") | Some("visionos-simulator")) {
        return Err(anyhow!(
            "Geisterhand is not supported on visionOS yet."
        ));
    }
    // Determine which UI crate to build based on target platform
    let ui_crate = match target {
        Some("ios-simulator") | Some("ios") => "perry-ui-ios",
        Some("android") => "perry-ui-android",
        Some("linux") => "perry-ui-gtk4",
        Some("windows") => "perry-ui-windows",
        _ if cfg!(target_os = "linux") => "perry-ui-gtk4",
        _ if cfg!(target_os = "windows") => "perry-ui-windows",
        _ => "perry-ui-macos",
    };

    match format {
        OutputFormat::Text => println!("Building geisterhand libraries ({}, {})...", ui_crate,
            rust_target_triple(target).unwrap_or("host")),
        OutputFormat::Json => {}
    }

    // Find the Perry workspace root by looking for Cargo.toml with [workspace]
    // relative to the perry executable
    let workspace_root = find_perry_workspace_root()
        .ok_or_else(|| anyhow!(
            "Cannot auto-build geisterhand libraries: Perry workspace not found.\n\
            Build manually from the Perry source directory:\n  \
            CARGO_TARGET_DIR=target/geisterhand cargo build --release \\\n    \
            -p perry-runtime --features geisterhand \\\n    \
            -p {} --features geisterhand \\\n    \
            -p perry-ui-geisterhand", ui_crate
        ))?;

    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd
        .current_dir(&workspace_root)
        .env("CARGO_TARGET_DIR", workspace_root.join("target/geisterhand"))
        .arg("build")
        .arg("--release")
        .arg("-p").arg("perry-runtime").arg("--features").arg("perry-runtime/geisterhand")
        .arg("-p").arg(ui_crate).arg("--features").arg(format!("{}/geisterhand", ui_crate))
        .arg("-p").arg("perry-ui-geisterhand");

    // Add cross-compilation target if needed
    if let Some(triple) = rust_target_triple(target) {
        cargo_cmd.arg("--target").arg(triple);
    }

    let status = cargo_cmd.status()
        .map_err(|e| anyhow!("Failed to run cargo: {}", e))?;

    if !status.success() {
        return Err(anyhow!("Failed to build geisterhand libraries (cargo exited with {})", status));
    }

    match format {
        OutputFormat::Text => println!("Geisterhand libraries built successfully"),
        OutputFormat::Json => {}
    }
    Ok(())
}

/// A pair of (runtime, stdlib) static libraries built with the auto-mode
/// chosen profile (custom feature set, optional `panic = "abort"`).
#[derive(Debug, Clone)]

#[cfg(all(test, target_os = "windows"))]
mod windows_toolchain_tests {
    use super::msvc_vswhere_installation_path_args;

    #[test]
    fn vswhere_query_requires_msvc_tools_component() {
        assert_eq!(
            msvc_vswhere_installation_path_args(),
            [
                "-products",
                "*",
                "-requires",
                "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                "-latest",
                "-property",
                "installationPath",
                "-nologo",
            ]
        );
    }
}
