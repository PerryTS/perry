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
//!   `find_ui_library`).
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
use std::sync::OnceLock;

use crate::OutputFormat;

// `rust_target_triple` and `find_perry_workspace_root` still live in
// the compile.rs orchestrator. Pull them in as private parent-module
// items so the search helpers below can reach them.
use super::{find_perry_workspace_root, rust_target_triple};

/// Resolve the host's Rust target triple by parsing `rustc -vV`.
///
/// Cached per-process via `OnceLock`. Returns `None` if `rustc` is missing
/// or its `-vV` output doesn't include a `host:` line — callers should
/// fall back to the simple `target/release/` layout in that case.
///
/// Used by `locate_native_lib_artifact` (refs #564) to probe
/// `target/<host-triple>/release/` when cargo writes artifacts under the
/// triple-prefixed directory because something pinned a default target
/// (`[build] target = "..."` in `.cargo/config.toml`,
/// `CARGO_BUILD_TARGET`, `rust-toolchain.toml` `targets = [...]`).
pub(crate) fn host_target_triple() -> Option<&'static str> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let vv = Command::new("rustc").arg("-vV").output().ok()?;
            if !vv.status.success() {
                return None;
            }
            let vv_str = String::from_utf8_lossy(&vv.stdout);
            let host_line = vv_str.lines().find(|l| l.starts_with("host:"))?;
            let triple = host_line.trim_start_matches("host:").trim().to_string();
            if triple.is_empty() {
                None
            } else {
                Some(triple)
            }
        })
        .as_deref()
}

/// Locate a `perry.nativeLibrary` crate's build artifact, probing both
/// the bare `target/release/` and the triple-prefixed
/// `target/<triple>/release/` layouts.
///
/// Cargo writes to `target/<triple>/release/` (not `target/release/`)
/// whenever something pins a default target — `[build] target = "..."`
/// in `.cargo/config.toml`, `CARGO_BUILD_TARGET`, or a
/// `rust-toolchain.toml` with `targets = [...]`. These setups are common
/// on Linux dev machines and CI, so a native build (no `--target` passed
/// to perry) needs to find the artifact in either location.
///
/// When a `--target` was passed, prefer the cross-target triple subdir
/// but fall through to `target/release/` defensively. When no target was
/// passed, prefer `target/release/` and fall through to the host triple.
///
/// Refs #564.
pub(crate) fn locate_native_lib_artifact(
    crate_target_dir: &Path,
    target: Option<&str>,
    lib_name: &str,
) -> Option<PathBuf> {
    let mut release_dirs: Vec<PathBuf> = Vec::new();
    if let Some(triple) = rust_target_triple(target) {
        release_dirs.push(crate_target_dir.join(triple).join("release"));
        release_dirs.push(crate_target_dir.join("release"));
    } else {
        release_dirs.push(crate_target_dir.join("release"));
        if let Some(host) = host_target_triple() {
            release_dirs.push(crate_target_dir.join(host).join("release"));
        }
    }
    for dir in &release_dirs {
        for name in lib_name_variants(lib_name, target) {
            let path = dir.join(&name);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

/// Expand a bare crate name into platform-appropriate staticlib /
/// dylib filenames. The literal name is tried first so manifests that
/// already include the full filename (e.g. `libfoo.a`) keep working
/// unchanged; the variants cover wrappers that supply only the bare
/// cargo lib name (refs issue #792).
fn lib_name_variants(lib_name: &str, target: Option<&str>) -> Vec<String> {
    let mut out = vec![lib_name.to_string()];
    let is_windows = matches!(target, Some("windows") | Some("windows-winui"))
        || (target.is_none() && cfg!(target_os = "windows"));
    let is_macos = matches!(
        target,
        Some("ios")
            | Some("ios-simulator")
            | Some("tvos")
            | Some("tvos-simulator")
            | Some("watchos")
            | Some("watchos-simulator")
            | Some("visionos")
            | Some("visionos-simulator")
            | Some("macos")
    ) || (target.is_none() && cfg!(target_os = "macos"));

    if std::path::Path::new(lib_name).extension().is_some() {
        // A wrapper may hard-code the Unix static-lib filename (e.g.
        // `libperry_ext_webgpu.a`) in its manifest `lib` field even when
        // targeting Windows, where cargo actually emits
        // `perry_ext_webgpu.lib` (MSVC drops the `lib` prefix and uses
        // the `.lib` extension). When the declared name carries the `.a`
        // extension but we're building for Windows, also probe the
        // MSVC-translated names so the artifact resolves without forcing
        // the wrapper to special-case the filename per platform. Refs #5812.
        if is_windows && std::path::Path::new(lib_name).extension() == Some("a".as_ref()) {
            let stem = lib_name.strip_suffix(".a").unwrap_or(lib_name);
            out.push(format!("{}.lib", stem));
            if let Some(stripped) = stem.strip_prefix("lib") {
                out.push(format!("{}.lib", stripped));
            }
        }
        return out;
    }

    // MSVC staticlib has no `lib` prefix — the cargo crate name *is* the
    // filename stem. Try the literal name first (covers crates legitimately
    // named `libfoo` → `libfoo.lib`), then the stripped variant so a
    // wrapper that copied the macOS convention into `"lib"` (e.g.
    // `libperry_ext_foo`) still resolves.
    if is_windows {
        out.push(format!("{}.lib", lib_name));
        if let Some(stripped) = lib_name.strip_prefix("lib") {
            out.push(format!("{}.lib", stripped));
        }
        return out;
    }

    // Unix-like: cargo prepends `lib` to staticlib/dylib output. Try the
    // prefixed form first; if `lib_name` already starts with `lib` we
    // also try without it so both conventions resolve.
    let prefixed = if lib_name.starts_with("lib") {
        lib_name.to_string()
    } else {
        format!("lib{}", lib_name)
    };
    let exts: &[&str] = if is_macos {
        &["a", "dylib"]
    } else {
        &["a", "so"]
    };
    for ext in exts {
        out.push(format!("{}.{}", prefixed, ext));
    }
    out
}

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
                    let exe_suffix = if cfg!(target_os = "windows") {
                        ".exe"
                    } else {
                        ""
                    };
                    let tool_path = PathBuf::from(&sysroot)
                        .join("lib")
                        .join("rustlib")
                        .join(host_triple)
                        .join("bin")
                        .join(format!("{}{}", tool_name, exe_suffix));
                    if tool_path.exists() {
                        return Some(tool_path);
                    }
                }
            }
        }
    }

    // 3. PATH lookup
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
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
///
/// `min_windows_version` accepts `"7"`, `"8"`, or `"10"` (default). Per the
/// PE subsystem ABI: `,5.1` = Win7-compatible, `,6.02` = Win8-compatible,
/// no suffix = linker default (Win8+ on modern toolchains). The PE subsystem
/// version is just the loader-side declaration of "this binary claims to run
/// on this version" — the binary still has to actually avoid calling APIs
/// newer than that version. Perry's UI runtime handles the API side via
/// `crates/perry-ui-windows/src/dpi_compat.rs` (issue #303).
/// Fold the resolved `--windows-subsystem` / `[windows] subsystem` override
/// (`ctx.windows_subsystem`) into the auto-detected `needs_ui` to get the
/// effective "is this a GUI app?" bool that `windows_pe_subsystem_flag`
/// consumes. `"windows"` forces GUI (`/SUBSYSTEM:WINDOWS`, no console window),
/// `"console"` forces a console, `"auto"` (and any unrecognized value — the
/// caller validates) defers to the import-driven heuristic.
pub(super) fn windows_subsystem_needs_ui(subsystem: &str, needs_ui: bool) -> bool {
    match subsystem {
        "windows" => true,
        "console" => false,
        _ => needs_ui,
    }
}

/// The conventional output-file extension for a Windows target, by output
/// type (#4771): executables are `.exe`, shared libraries `.dll`, static
/// libraries `.lib`. Used to default the extension on a Windows `-o NAME`
/// that the user gave without one, so the produced file is runnable from
/// PowerShell/cmd (which won't launch an extension-less file) and linkable
/// under the platform's library conventions.
pub(super) fn windows_default_output_extension(is_dylib: bool, is_staticlib: bool) -> &'static str {
    if is_dylib {
        "dll"
    } else if is_staticlib {
        "lib"
    } else {
        "exe"
    }
}

pub(super) fn windows_pe_subsystem_flag(needs_ui: bool, min_windows_version: &str) -> String {
    let base = if needs_ui {
        "/SUBSYSTEM:WINDOWS"
    } else {
        "/SUBSYSTEM:CONSOLE"
    };
    match min_windows_version {
        "7" => format!("{},5.1", base),
        "8" => format!("{},6.02", base),
        // "10" or anything else (caller is expected to validate) — no suffix,
        // linker picks its default. Preserves current behavior for users
        // who don't pass --min-windows-version.
        _ => base.to_string(),
    }
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
            if crt.exists() {
                paths.push(crt.to_string_lossy().to_string());
            }
            if um.exists() {
                paths.push(um.to_string_lossy().to_string());
            }
            if ucrt.exists() {
                paths.push(ucrt.to_string_lossy().to_string());
            }
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

    // Find Windows SDK lib paths.
    //
    // Issue #300: pre-fix this hardcoded `C:\Program Files (x86)\Windows
    // Kits\10\Lib` and silently returned only the MSVC CRT path (so `LIB`
    // was missing `um\x64` → `LNK1181: cannot open user32.lib`) when the
    // user had Windows SDK installed elsewhere — typical for non-default
    // VS installs (D: drive, custom paths). We now probe a list of
    // candidate roots in priority order:
    //
    //   1. Registry: HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots
    //      value KitsRoot10 — this is what `vcvars64.bat` consults and
    //      is the canonical source of truth for SDK location.
    //   2. ProgramFiles env (handles arch-specific %ProgramFiles%).
    //   3. ProgramFiles(x86) env.
    //   4. Hardcoded fallback at the legacy default path.
    //
    // Each root is `<root>\Windows Kits\10\Lib` (or for the registry's
    // KitsRoot10, just `<KitsRoot10>\Lib`).
    let mut sdk_roots: Vec<PathBuf> = Vec::new();
    if let Some(reg_root) = read_registry_kits_root_10() {
        sdk_roots.push(reg_root.join("Lib"));
    }
    for env_var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(pf) = std::env::var(env_var) {
            sdk_roots.push(PathBuf::from(pf).join(r"Windows Kits\10\Lib"));
        }
    }
    sdk_roots.push(PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\Lib"));

    let mut sdk_added = false;
    for sdk_root in &sdk_roots {
        if let Ok(entries) = std::fs::read_dir(sdk_root) {
            let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
            if let Some(entry) = versions.first() {
                let um_path = entry.path().join(r"um\x64");
                let ucrt_path = entry.path().join(r"ucrt\x64");
                if um_path.exists() {
                    paths.push(um_path.to_string_lossy().to_string());
                    sdk_added = true;
                }
                if ucrt_path.exists() {
                    paths.push(ucrt_path.to_string_lossy().to_string());
                }
                if sdk_added {
                    break;
                }
            }
        }
    }

    if !sdk_added && std::env::var("LIB").is_err() {
        // Loud diagnostic — pre-fix this returned silently with only the
        // MSVC CRT path, leading to a confusing LNK1181 from link.exe
        // about user32.lib. Tell the user exactly what we tried and what
        // the workarounds are.
        eprintln!(
            "Warning: Windows SDK lib path (Windows Kits\\10\\Lib\\<ver>\\um\\x64) not found.\n\
             Tried: {}\n\
             Linker will likely fail with LNK1181 (e.g. cannot open user32.lib).\n\
             Workarounds:\n\
             - Run `vcvars64.bat` before `perry compile` (sets `LIB` env)\n\
             - Install Windows 10/11 SDK via Visual Studio Installer\n\
             - Set the `LIB` env var manually to your SDK's `um\\x64;ucrt\\x64` paths",
            sdk_roots
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    if paths.is_empty() {
        None
    } else {
        Some(paths.join(";"))
    }
}

/// Issue #300: read `KitsRoot10` from the Windows registry so we don't
/// hardcode the SDK install location. Returns the path that
/// `vcvars64.bat` would consult. Best-effort — silently returns None
/// on any error (registry not available, key missing, etc.).
#[cfg(target_os = "windows")]
fn read_registry_kits_root_10() -> Option<PathBuf> {
    use std::process::Command;
    // We could pull in the `winreg` crate, but a `reg query` shell-out
    // keeps the perry build dep-free for the same lookup. Output shape:
    //     HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows Kits\Installed Roots
    //         KitsRoot10    REG_SZ    C:\Program Files (x86)\Windows Kits\10\
    let out = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots",
            "/v",
            "KitsRoot10",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("KitsRoot10") {
            // Skip whitespace + REG_SZ + whitespace, take the rest.
            let rest = rest.trim_start();
            let rest = rest.strip_prefix("REG_SZ").unwrap_or(rest).trim();
            if !rest.is_empty() {
                let p = PathBuf::from(rest.trim_end_matches('\\'));
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

// #854: cfg-symmetric stub — the Windows build calls this (line ~482); the
// non-Windows build keeps the same signature so callers compile cross-platform.
#[allow(dead_code)]
#[cfg(not(target_os = "windows"))]
fn read_registry_kits_root_10() -> Option<PathBuf> {
    None
}

#[cfg(not(target_os = "windows"))]
pub(super) fn find_msvc_lib_paths() -> Option<String> {
    let sysroot = std::env::var("PERRY_WINDOWS_SYSROOT").ok()?;
    let root = PathBuf::from(&sysroot);
    if !root.exists() {
        eprintln!(
            "Warning: PERRY_WINDOWS_SYSROOT={} does not exist",
            root.display()
        );
        return None;
    }

    Some(xwin_sysroot_lib_paths(&root).join(";"))
}

/// Find a library by name, optionally searching cross-compilation target directories.
///
/// Returns the located path, or a list of all searched candidate paths so the
/// caller can surface them in an error message.
pub(super) fn find_library_with_candidates(
    name: &str,
    target: Option<&str>,
) -> Result<PathBuf, Vec<PathBuf>> {
    let candidates = collect_library_candidates(name, target);
    for path in &candidates {
        if path.exists() {
            return Ok(path.clone());
        }
        // npm per-platform packages ship `*.a.zst` (the raw archives exceed
        // npm's tarball upload limit). When only the compressed sibling is
        // present, decompress it once into a per-user cache and link that.
        let compressed = super::compressed_libs::compressed_sibling(path);
        if compressed.exists() {
            let lib_name = path.file_name().and_then(|s| s.to_str()).unwrap_or(name);
            match super::compressed_libs::decompressed_archive(&compressed, lib_name) {
                Ok(decompressed) => return Ok(decompressed),
                // A compressed archive is present but couldn't be expanded
                // (corrupt download, out of disk, …). Surface the real cause
                // loudly here — otherwise it's masked by the generic "library
                // not found" error the caller raises after exhausting candidates.
                Err(e) => eprintln!(
                    "  error: failed to decompress {}: {:#}",
                    compressed.display(),
                    e
                ),
            }
        }
    }
    Err(candidates)
}

pub fn find_library(name: &str, target: Option<&str>) -> Option<PathBuf> {
    find_library_with_candidates(name, target).ok()
}

/// Probe WinGet's Packages directory for a library file. WinGet stores
/// `perry.exe` and the `.lib` files together in
/// `%LOCALAPPDATA%\Microsoft\WinGet\Packages\PerryTS.Perry_<source-hash>\`,
/// but exposes the binary via a launcher-shim `WinGet\Links\perry.exe`.
/// The shim is a launcher .exe rather than a symlink, so `current_exe()`
/// returns the shim path and the existing `dir.join(name)` lookups land
/// in the wrong place. Closes #352.
#[cfg(target_os = "windows")]
fn winget_lib_candidates(name: &str) -> Vec<PathBuf> {
    let Ok(local_app_data) = std::env::var("LOCALAPPDATA") else {
        return Vec::new();
    };
    let packages = PathBuf::from(local_app_data)
        .join("Microsoft")
        .join("WinGet")
        .join("Packages");
    let Ok(entries) = std::fs::read_dir(&packages) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("PerryTS.Perry_"))
        {
            out.push(path.join(name));
        }
    }
    out
}

#[cfg(not(target_os = "windows"))]
fn winget_lib_candidates(_name: &str) -> Vec<PathBuf> {
    Vec::new()
}

/// Compose the platform-suffixed name for an Apple / HarmonyOS cross-compile
/// lib in a flat install dir (Homebrew bottle, hand-staged install, etc.).
///
/// Inputs:
/// - `name`: the canonical lib filename cargo emits (e.g. `libperry_ui_ios.a`,
///   `libperry_runtime.a`).
/// - `class`: the platform class suffix, with leading underscore
///   (`"_ios"` / `"_tvos"` / etc.).
/// - `is_sim`: whether this is the simulator variant — appends `_sim` before
///   `.a` so device + sim libs can coexist in the same dir without colliding.
///
/// The composition rule:
/// - If the stem already ends with `class` (e.g. `libperry_ui_ios` for `_ios`),
///   only append the variant: `libperry_ui_ios.a` / `libperry_ui_ios_sim.a`.
/// - Otherwise, append both class + variant: `libperry_runtime_ios.a` /
///   `libperry_runtime_ios_sim.a`.
///
/// Used by the cross-compile candidate list in `collect_library_candidates`.
fn apple_class_lib_name(name: &str, class: &str, is_sim: bool) -> String {
    let variant_suffix = if is_sim { "_sim" } else { "" };
    if let Some(stem) = name.strip_suffix(".a") {
        if stem.ends_with(class) {
            format!("{}{}.a", stem, variant_suffix)
        } else {
            format!("{}{}{}.a", stem, class, variant_suffix)
        }
    } else {
        // Non-`.a` (Windows-style names shouldn't hit this branch — the
        // cross-compile callers above only fire for Unix targets).
        name.to_string()
    }
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
        if matches!(target, Some("windows") | Some("windows-winui")) {
            candidates.push(PathBuf::from(format!("target/release/{}", name)));
            candidates.push(PathBuf::from(format!("target/debug/{}", name)));
            candidates.extend(winget_lib_candidates(name));
        }
        #[cfg(target_os = "linux")]
        if matches!(target, Some("linux")) {
            candidates.push(PathBuf::from(format!("target/release/{}", name)));
            candidates.push(PathBuf::from(format!("target/debug/{}", name)));
        }
        // Also check directories relative to the perry executable.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                // #872: WinGet extracts the package zip into a single
                // `PerryTS.Perry_<hash>` directory under
                // `%LOCALAPPDATA%\Microsoft\WinGet\Packages\`. The
                // package staging (release-packages.yml) writes Android
                // cross-compile libs at `<root>/aarch64-linux-android/
                // release/<name>` inside that same dir, so this lookup
                // — `dir/<triple>/release/<name>` — must come BEFORE the
                // sibling `dir.parent()` path below or the existing
                // search would walk up and miss the staged libs.
                candidates.push(dir.join(triple).join("release").join(name));
                candidates.push(dir.join(triple).join("debug").join(name));
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
                    .join("../../target")
                    .join(triple)
                    .join("release")
                    .join(name);
                candidates.push(source_target);

                // For Apple / HarmonyOS cross-compile targets, check the exe
                // directory for libs with the platform-suffix naming convention:
                // - Libs already named with the class suffix (e.g. libperry_ui_ios.a) → direct
                // - Other libs (e.g. libperry_runtime.a stored as libperry_runtime_ios.a)
                //
                // Closes #394: also probe `<prefix>/lib/<suffixed-name>` so a
                // Homebrew-installed bottle (binary at `<prefix>/bin/perry`,
                // libs at `<prefix>/lib/`) resolves cross-compile libs the
                // same way the host-build branch already does.
                //
                // Device + simulator share the same canonical lib name (e.g.
                // `libperry_ui_ios.a` is what cargo emits for both
                // `aarch64-apple-ios` and `aarch64-apple-ios-sim`) — fine in
                // dev because the triple-specific candidates above isolate
                // them, but they collide in a flat lib dir like Homebrew's
                // `<prefix>/lib/`. Differentiate with a `_sim` suffix BEFORE
                // `.a` (e.g. `libperry_ui_ios_sim.a` for the simulator
                // variant) so both can coexist in the bottle. The sim-only
                // v0.5.470 fix shipped only the sim variant and named it
                // `libperry_ui_ios.a` (same name as device); v0.5.472+ ships
                // both and uses this suffix to disambiguate.
                let class_and_sim = match target {
                    Some("ios") | Some("ios-widget") => Some(("_ios", false)),
                    Some("ios-simulator") | Some("ios-widget-simulator") => Some(("_ios", true)),
                    Some("visionos") => Some(("_visionos", false)),
                    Some("visionos-simulator") => Some(("_visionos", true)),
                    Some("watchos") => Some(("_watchos", false)),
                    Some("watchos-simulator") => Some(("_watchos", true)),
                    Some("tvos") => Some(("_tvos", false)),
                    Some("tvos-simulator") => Some(("_tvos", true)),
                    Some("harmonyos") => Some(("_harmonyos", false)),
                    Some("harmonyos-simulator") => Some(("_harmonyos", true)),
                    _ => None,
                };
                if let Some((class, is_sim)) = class_and_sim {
                    let suffixed = apple_class_lib_name(name, class, is_sim);
                    candidates.push(dir.join(&suffixed));
                    if let Some(prefix) = dir.parent() {
                        candidates.push(prefix.join("lib").join(&suffixed));
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
            .join("../../target/release")
            .join(name);
        candidates.push(source_target);
        candidates.push(PathBuf::from(format!("/usr/local/lib/{}", name)));
        // Debian/Ubuntu: libs installed in /usr/lib/perry
        candidates.push(PathBuf::from(format!("/usr/lib/perry/{}", name)));
        candidates.extend(winget_lib_candidates(name));
    }

    candidates
}

/// Find the runtime library for linking
pub(super) fn find_runtime_library(target: Option<&str>) -> Result<PathBuf> {
    let lib_name = match target {
        Some("windows") | Some("windows-winui") => "perry_runtime.lib",
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

/// Find the panic=abort prebuilt runtime variant (optional — shipped by
/// release packaging for runtime-only apps; selected by the out-of-tree
/// fallback in `optimized_libs.rs` when no `catch_unwind` callers are
/// reachable and stdlib is not linked). Unix-only: Windows always links
/// stdlib, which is built panic=unwind.
pub(super) fn find_runtime_abort_library(target: Option<&str>) -> Option<PathBuf> {
    if matches!(target, Some("windows") | Some("windows-winui")) {
        return None;
    }
    #[cfg(target_os = "windows")]
    if target.is_none() {
        return None;
    }
    find_library("libperry_runtime_abort.a", target)
}

/// Find the stdlib library for linking (optional - only needed for native modules)
pub(super) fn find_stdlib_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("windows") | Some("windows-winui") => "perry_stdlib.lib",
        #[cfg(target_os = "windows")]
        None => "perry_stdlib.lib",
        _ => "libperry_stdlib.a",
    };
    find_library(lib_name, target)
}

/// Find the wasmi-based WebAssembly host library (optional — only needed
/// when `--enable-wasm-runtime` is set, see issue #76).
pub(super) fn find_wasm_host_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("windows") | Some("windows-winui") => "perry_wasm_host.lib",
        #[cfg(target_os = "windows")]
        None => "perry_wasm_host.lib",
        _ => "libperry_wasm_host.a",
    };
    find_library(lib_name, target)
}

/// Find the UI library for linking (optional - only needed when perry/ui is imported).
///
/// HarmonyOS is intentionally absent: there is no `perry-ui-harmonyos`
/// crate by design — UI is emitted as ArkUI source via the codegen-arkts
/// harvest, and any `perry_ui_*` / `perry_system_*` / `perry_updater_*`
/// symbols that survive into the .so resolve via the no-op stubs auto-
/// generated by `perry-runtime/build.rs` (#395 + #399). The harmonyos
/// branch in `compile.rs` unconditionally clears `ctx.needs_ui` for that
/// target so this lookup is never reached with `Some("harmonyos*")`
/// (#400).
pub(super) fn find_ui_library(target: Option<&str>) -> Option<PathBuf> {
    let lib_name = match target {
        Some("ios-simulator") | Some("ios") => "libperry_ui_ios.a",
        Some("visionos-simulator") | Some("visionos") => "libperry_ui_visionos.a",
        // Wear OS reuses the Android View backend.
        Some("android") | Some("wearos") => "libperry_ui_android.a",
        Some("watchos-simulator") | Some("watchos") => "libperry_ui_watchos.a",
        Some("tvos-simulator") | Some("tvos") => "libperry_ui_tvos.a",
        Some("linux") => "libperry_ui_gtk4.a",
        Some("macos") => "libperry_ui_macos.a",
        // Opt-in WinUI 3 backend (#4680) — its own staticlib. It bundles the
        // perry-ui-windows Win32 symbols today (scaffold), so the FFI surface
        // is identical to the `windows` lib.
        Some("windows-winui") => "perry_ui_windows_winui.lib",
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
            "/Applications/DevEco-Studio.app/Contents/sdk/default/openharmony/native",
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

/// Build the `CC_*` / `CXX_*` / `CARGO_TARGET_*_LINKER` env vars that
/// point cc-rs and rustc at the Android NDK toolchain. Issue #1508 —
/// without these, `cargo build --target aarch64-linux-android` falls
/// back to the host `cc`, which fails outright on Windows (`clang.exe
/// not found`) and produces architecturally-mismatched objects on Unix.
///
/// Mirrors `harmonyos_cross_env` below. The NDK API level is fixed at
/// 24 (Android 7.0) — matches the existing `platform_cmd.rs` /
/// `link/mod.rs` JNI stub compile invocations.
pub(super) fn android_cross_env(ndk_home: &Path, target: Option<&str>) -> Vec<(String, String)> {
    let (triple, clang_target) = match target {
        Some("android-x86_64") => ("x86_64-linux-android", "x86_64-linux-android24"),
        // `android` (default) is the arm64 device target.
        _ => ("aarch64-linux-android", "aarch64-linux-android24"),
    };

    // NDK ships per-host prebuilt toolchains. Tag must match the build
    // machine, NOT the target — Windows-host builds were falling through
    // to `linux-x86_64` and erroring out before this fix landed.
    let host_tag = if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    };

    let bin = ndk_home
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(host_tag)
        .join("bin");
    // On Windows the NDK's wrapper scripts are `.cmd` files.
    let ext = if cfg!(target_os = "windows") {
        ".cmd"
    } else {
        ""
    };
    let clang = bin.join(format!("{}-clang{}", clang_target, ext));
    let clangpp = bin.join(format!("{}-clang++{}", clang_target, ext));
    // NDK r27+ removed the per-target `aarch64-linux-android-ar` wrapper that
    // cc-rs probes for by default; the archiver is now the unprefixed
    // `llvm-ar`. Without an explicit `AR_<triple>` the runtime/stdlib C
    // dependencies (e.g. mimalloc) fail to build with
    // `failed to find tool "aarch64-linux-android-ar"`. `llvm-ar` has no `.cmd`
    // wrapper on Windows — it's the bare executable (+`.exe`).
    let ar_ext = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let llvm_ar = bin.join(format!("llvm-ar{}", ar_ext));

    let triple_upper = triple.to_uppercase().replace('-', "_");
    let triple_under = triple.replace('-', "_");

    vec![
        (format!("CC_{}", triple), clang.display().to_string()),
        (format!("CC_{}", triple_under), clang.display().to_string()),
        (format!("CXX_{}", triple), clangpp.display().to_string()),
        (
            format!("CXX_{}", triple_under),
            clangpp.display().to_string(),
        ),
        (format!("AR_{}", triple), llvm_ar.display().to_string()),
        (
            format!("AR_{}", triple_under),
            llvm_ar.display().to_string(),
        ),
        (
            format!("CARGO_TARGET_{}_LINKER", triple_upper),
            clang.display().to_string(),
        ),
    ]
}

/// Cross-compile env vars to pass to `cargo build` so `cc-rs` picks up the
/// OHOS SDK's clang + musl sysroot for any C source in dependency build.rs
/// scripts (notably `libmimalloc-sys`, which needs `pthread.h`).
///
/// Cargo reads both `CC_<triple>` and the underscored `CC_<TRIPLE>` form —
/// `cc-rs` prefers the latter. We set both for robustness. Same for linker.
pub(super) fn harmonyos_cross_env(
    sdk_native: &Path,
    target: Option<&str>,
) -> Vec<(String, String)> {
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
        (
            format!("CXX_{}", triple_under),
            clangpp.display().to_string(),
        ),
        (format!("CFLAGS_{}", triple), cflags.clone()),
        (format!("CFLAGS_{}", triple_under), cflags.clone()),
        (format!("CXXFLAGS_{}", triple), cflags.clone()),
        (format!("CXXFLAGS_{}", triple_under), cflags),
        (
            format!("CARGO_TARGET_{}_LINKER", triple_upper),
            clang.display().to_string(),
        ),
        (
            format!("CARGO_TARGET_{}_RUSTFLAGS", triple_upper),
            rustflags,
        ),
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
    // When a cross-compile target is specified, ONLY look in that triple's
    // release directories. Falling through to host `target/release/` returns
    // a wrong-architecture `.a` that the linker then refuses with
    //
    //   ld: building for 'iOS-simulator', but linking in object file
    //   (target/release/libperry_runtime.a[2]…) built for 'macOS'
    //
    // Pre-fix the host fallback also masked the "auto-build geisterhand for
    // this target" check in `link/mod.rs` — `find_geisterhand_runtime`
    // returned the host `.a`, `gh_missing` stayed `false`, and
    // `build_geisterhand_libs(<cross-target>)` never fired. Surfaced on the
    // v0.5.1024 simctl-tests workflow run.
    let triple = rust_target_triple(target);
    for root in &search_roots {
        if let Some(triple) = triple {
            // Separate geisterhand build dir
            let path = root.join(format!("target/geisterhand/{}/release/{}", triple, name));
            if path.exists() {
                return Some(path);
            }
            // Shared release dir (when built with --features geisterhand in normal target)
            let path = root.join(format!("target/{}/release/{}", triple, name));
            if path.exists() {
                return Some(path);
            }
            // No host fallback for cross-compile targets — see comment above.
            continue;
        }
        // Host build (target is None / matches host triple) — search both
        // geisterhand-specific and shared release directories.
        let path = root.join(format!("target/geisterhand/release/{}", name));
        if path.exists() {
            return Some(path);
        }
        let path = root.join(format!("target/release/{}", name));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

pub(super) fn find_geisterhand_library(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("windows") | Some("windows-winui"))
        || cfg!(target_os = "windows")
    {
        "perry_ui_geisterhand.lib"
    } else {
        "libperry_ui_geisterhand.a"
    };
    find_geisterhand_lib(name, target).or_else(|| find_library(name, None))
}

pub(super) fn find_geisterhand_runtime(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("windows") | Some("windows-winui"))
        || cfg!(target_os = "windows")
    {
        "perry_runtime.lib"
    } else {
        "libperry_runtime.a"
    };
    find_geisterhand_lib(name, target)
}

pub(super) fn find_geisterhand_stdlib(target: Option<&str>) -> Option<PathBuf> {
    // The geisterhand auto-build (`build_geisterhand_libs`) compiles
    // perry-stdlib into target/geisterhand alongside the geisterhand-featured
    // perry-runtime, so this archive carries perry-stdlib's full default
    // feature set (incl. `async-runtime` → the `perry_ffi_promise_*` shims)
    // and shares a hash-consistent `perry-runtime` with `find_geisterhand_runtime`.
    // Only the geisterhand build dirs are searched (no `find_library` fallback)
    // so we never select the auto-optimized stdlib, whose feature set is
    // computed from the app's TS imports and omits async-runtime when the async
    // surface comes from a native binding — the #1383 link failure.
    let name = if matches!(target, Some("windows") | Some("windows-winui"))
        || cfg!(target_os = "windows")
    {
        "perry_stdlib.lib"
    } else {
        "libperry_stdlib.a"
    };
    find_geisterhand_lib(name, target)
}

pub(super) fn find_geisterhand_ui(target: Option<&str>) -> Option<PathBuf> {
    let name = if matches!(target, Some("ios-simulator") | Some("ios")) {
        "libperry_ui_ios.a"
    } else if matches!(target, Some("visionos-simulator") | Some("visionos")) {
        return None;
    } else if matches!(target, Some("android") | Some("wearos")) {
        "libperry_ui_android.a"
    } else if matches!(target, Some("linux")) || cfg!(target_os = "linux") {
        "libperry_ui_gtk4.a"
    } else if matches!(target, Some("windows-winui")) {
        "perry_ui_windows_winui.lib"
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
        return Err(anyhow!("Geisterhand is not supported on visionOS yet."));
    }
    // Determine which UI crate to build based on target platform
    let ui_crate = match target {
        Some("ios-simulator") | Some("ios") => "perry-ui-ios",
        Some("android") | Some("wearos") => "perry-ui-android",
        Some("linux") => "perry-ui-gtk4",
        Some("windows-winui") => "perry-ui-windows-winui",
        Some("windows") => "perry-ui-windows",
        _ if cfg!(target_os = "linux") => "perry-ui-gtk4",
        _ if cfg!(target_os = "windows") => "perry-ui-windows",
        _ => "perry-ui-macos",
    };

    match format {
        OutputFormat::Text => println!(
            "Building geisterhand libraries ({}, {})...",
            ui_crate,
            rust_target_triple(target).unwrap_or("host")
        ),
        OutputFormat::Json => {}
    }

    // Find the Perry workspace root by looking for Cargo.toml with [workspace]
    // relative to the perry executable
    let workspace_root = find_perry_workspace_root().ok_or_else(|| {
        anyhow!(
            "Cannot auto-build geisterhand libraries: Perry workspace not found.\n\
            Build manually from the Perry source directory:\n  \
            CARGO_TARGET_DIR=target/geisterhand cargo build --release \\\n    \
            -p perry-runtime-static --features perry-runtime/geisterhand \\\n    \
            -p {} --features geisterhand \\\n    \
            -p perry-ui-geisterhand",
            ui_crate
        )
    })?;

    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd
        .current_dir(&workspace_root)
        .env(
            "CARGO_TARGET_DIR",
            workspace_root.join("target/geisterhand"),
        )
        .arg("build")
        .arg("--release")
        // #5422 — staticlib (.a) now comes from the *-static wrapper crates;
        // perry-runtime/perry-stdlib are rlib-only. The `perry-runtime/<feat>`
        // selector still resolves via the wrapper's dependency graph.
        .arg("-p")
        .arg("perry-runtime-static")
        .arg("--features")
        .arg("perry-runtime/geisterhand")
        .arg("-p")
        .arg(ui_crate)
        .arg("--features")
        .arg(format!("{}/geisterhand", ui_crate))
        .arg("-p")
        .arg("perry-ui-geisterhand")
        // Build perry-stdlib in the same invocation so the geisterhand lib
        // set is complete and its shared `perry-runtime` is hash-consistent
        // with the runtime/ui libs above (a separate `cargo build -p
        // perry-stdlib` would resolve `perry-runtime` without the geisterhand
        // feature → a second runtime variant → undefined-symbol link errors).
        // Without stdlib here, apps that pull native-FFI packages depending on
        // perry-stdlib's async surface (`perry_ffi_promise_*`) had no
        // consistent stdlib to link against under --enable-geisterhand (#1383).
        .arg("-p")
        .arg("perry-stdlib-static");

    // Add cross-compilation target if needed
    if let Some(triple) = rust_target_triple(target) {
        cargo_cmd.arg("--target").arg(triple);
    }

    // #1529 — the geisterhand runtime/stdlib also land in the dlopen'd
    // `libperry_app.so` on Android; force global-dynamic TLS so the IE
    // model doesn't crash at load. (RUSTFLAGS scopes to the cross target,
    // so host build-scripts/proc-macros are unaffected.)
    if matches!(
        target,
        Some("android") | Some("android-x86_64") | Some("wearos")
    ) {
        let tls_flag = super::optimized_libs::android_global_dynamic_tls_rustflag(&mut cargo_cmd);
        cargo_cmd.env("RUSTFLAGS", tls_flag);
    }

    let status = cargo_cmd
        .status()
        .map_err(|e| anyhow!("Failed to run cargo: {}", e))?;

    if !status.success() {
        return Err(anyhow!(
            "Failed to build geisterhand libraries (cargo exited with {})",
            status
        ));
    }

    match format {
        OutputFormat::Text => println!("Geisterhand libraries built successfully"),
        OutputFormat::Json => {}
    }
    Ok(())
}

#[cfg(test)]
mod apple_lib_name_tests {
    use super::apple_class_lib_name;

    #[test]
    fn class_stem_device_uses_canonical_name() {
        // libperry_ui_ios.a stem already carries _ios → device adds nothing.
        assert_eq!(
            apple_class_lib_name("libperry_ui_ios.a", "_ios", false),
            "libperry_ui_ios.a"
        );
    }

    #[test]
    fn class_stem_sim_appends_sim_suffix() {
        // Same stem, simulator variant → _sim before .a.
        assert_eq!(
            apple_class_lib_name("libperry_ui_ios.a", "_ios", true),
            "libperry_ui_ios_sim.a"
        );
    }

    #[test]
    fn generic_stem_device_appends_class() {
        // libperry_runtime.a → device gets _ios appended.
        assert_eq!(
            apple_class_lib_name("libperry_runtime.a", "_ios", false),
            "libperry_runtime_ios.a"
        );
    }

    #[test]
    fn generic_stem_sim_appends_class_and_sim() {
        // libperry_runtime.a → simulator gets _ios_sim appended.
        assert_eq!(
            apple_class_lib_name("libperry_runtime.a", "_ios", true),
            "libperry_runtime_ios_sim.a"
        );
    }

    #[test]
    fn handles_other_class_suffixes() {
        // Spot-check non-iOS classes to make sure the helper isn't iOS-specific.
        assert_eq!(
            apple_class_lib_name("libperry_ui_tvos.a", "_tvos", true),
            "libperry_ui_tvos_sim.a"
        );
        assert_eq!(
            apple_class_lib_name("libperry_stdlib.a", "_visionos", false),
            "libperry_stdlib_visionos.a"
        );
    }
}

#[cfg(test)]
mod native_lib_artifact_tests {
    use super::{host_target_triple, locate_native_lib_artifact};
    use std::fs;

    /// Refs #564 — when cargo writes to `target/<triple>/release/`
    /// (because something pinned a default target), perry must still
    /// find the artifact for a native build (no `--target` passed).
    #[test]
    fn locates_artifact_under_host_triple_dir_for_native_build() {
        let host = match host_target_triple() {
            Some(h) => h,
            None => return, // rustc unavailable in this test env — skip.
        };

        let tmp = tempfile::tempdir().expect("create tmpdir");
        let target_dir = tmp.path().join("target");
        let triple_dir = target_dir.join(host).join("release");
        fs::create_dir_all(&triple_dir).expect("mkdir triple/release");
        let lib_path = triple_dir.join("libfoo.a");
        fs::write(&lib_path, b"fake archive").expect("write lib");

        let found = locate_native_lib_artifact(&target_dir, None, "libfoo.a");
        assert_eq!(found.as_deref(), Some(lib_path.as_path()));
    }

    #[test]
    fn prefers_bare_release_dir_when_present() {
        let tmp = tempfile::tempdir().expect("create tmpdir");
        let target_dir = tmp.path().join("target");
        let release_dir = target_dir.join("release");
        fs::create_dir_all(&release_dir).expect("mkdir release");
        let lib_path = release_dir.join("libfoo.a");
        fs::write(&lib_path, b"fake archive").expect("write lib");

        let found = locate_native_lib_artifact(&target_dir, None, "libfoo.a");
        assert_eq!(found.as_deref(), Some(lib_path.as_path()));
    }

    /// Refs #792 — wrappers that supply only the cargo crate name
    /// (e.g. `perry_ext_foo`) instead of the full filename should
    /// still resolve to `libperry_ext_foo.a` on the host platform.
    #[test]
    fn locates_artifact_from_bare_crate_name() {
        let tmp = tempfile::tempdir().expect("create tmpdir");
        let target_dir = tmp.path().join("target");
        let release_dir = target_dir.join("release");
        fs::create_dir_all(&release_dir).expect("mkdir release");
        let lib_name = if cfg!(target_os = "windows") {
            "perry_ext_foo.lib"
        } else {
            "libperry_ext_foo.a"
        };
        let lib_path = release_dir.join(lib_name);
        fs::write(&lib_path, b"fake archive").expect("write lib");

        let found = locate_native_lib_artifact(&target_dir, None, "perry_ext_foo");
        assert_eq!(found.as_deref(), Some(lib_path.as_path()));
    }

    /// On MSVC, cargo emits `{crate_name}.lib` literally — there is no
    /// automatic `lib` prefix. So a crate whose name actually starts
    /// with `lib` (e.g. `libfoo`) produces `libfoo.lib`, and Perry's
    /// variant logic must NOT strip the `lib` prefix in that case.
    #[test]
    #[cfg(target_os = "windows")]
    fn windows_preserves_lib_prefix_when_crate_name_starts_with_lib() {
        let tmp = tempfile::tempdir().expect("create tmpdir");
        let target_dir = tmp.path().join("target");
        let release_dir = target_dir.join("release");
        fs::create_dir_all(&release_dir).expect("mkdir release");
        let lib_path = release_dir.join("libfoo.lib");
        fs::write(&lib_path, b"fake archive").expect("write lib");

        let found = locate_native_lib_artifact(&target_dir, None, "libfoo");
        assert_eq!(found.as_deref(), Some(lib_path.as_path()));
    }

    /// Cross-platform check on the variant set itself so non-Windows
    /// CI also covers the `lib`-prefix preservation logic.
    #[test]
    fn variant_set_for_windows_target_keeps_lib_prefix() {
        let variants = super::lib_name_variants("libfoo", Some("windows"));
        assert!(
            variants.iter().any(|v| v == "libfoo.lib"),
            "expected libfoo.lib in {:?}",
            variants
        );
    }

    /// Refs #5812 — a wrapper that hard-codes the Unix static-lib
    /// filename (`libperry_ext_webgpu.a`) in its manifest must still
    /// resolve when targeting Windows, where cargo emits
    /// `perry_ext_webgpu.lib` (MSVC drops the `lib` prefix and the
    /// extension is `.lib`). The literal `.a` name is still tried first
    /// so Unix builds keep working unchanged.
    #[test]
    fn variant_set_translates_unix_static_lib_to_msvc_on_windows() {
        let variants = super::lib_name_variants("libperry_ext_webgpu.a", Some("windows"));
        assert_eq!(
            variants.first().map(String::as_str),
            Some("libperry_ext_webgpu.a")
        );
        assert!(
            variants.iter().any(|v| v == "perry_ext_webgpu.lib"),
            "expected perry_ext_webgpu.lib in {:?}",
            variants
        );
    }

    /// The `.a` → `.lib` translation is Windows-only; a Unix target must
    /// not start probing for `.lib` files.
    #[test]
    fn variant_set_does_not_translate_static_lib_on_unix() {
        let variants = super::lib_name_variants("libperry_ext_webgpu.a", Some("linux"));
        assert_eq!(variants, vec!["libperry_ext_webgpu.a".to_string()]);
    }

    /// End-to-end: a manifest declaring `libperry_ext_webgpu.a` resolves
    /// the on-disk `perry_ext_webgpu.lib` that cargo actually produced on
    /// Windows. Refs #5812.
    #[test]
    #[cfg(target_os = "windows")]
    fn locates_msvc_artifact_from_unix_manifest_name() {
        let tmp = tempfile::tempdir().expect("create tmpdir");
        let target_dir = tmp.path().join("target");
        let release_dir = target_dir.join("release");
        fs::create_dir_all(&release_dir).expect("mkdir release");
        let lib_path = release_dir.join("perry_ext_webgpu.lib");
        fs::write(&lib_path, b"fake archive").expect("write lib");

        let found = locate_native_lib_artifact(&target_dir, None, "libperry_ext_webgpu.a");
        assert_eq!(found.as_deref(), Some(lib_path.as_path()));
    }

    #[test]
    fn returns_none_when_artifact_missing() {
        let tmp = tempfile::tempdir().expect("create tmpdir");
        let target_dir = tmp.path().join("target");
        fs::create_dir_all(&target_dir).expect("mkdir target");
        let found = locate_native_lib_artifact(&target_dir, None, "libfoo.a");
        assert!(found.is_none());
    }
}

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
