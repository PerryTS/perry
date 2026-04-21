//! Build and run the executable link command.
//!
//! Tier 2.1 final extraction (v0.5.342) — moves the per-platform link command
//! construction out of `crates/perry/src/commands/compile.rs::run_with_parse_cache`.
//! Pre-extraction, the link logic was a ~1240-LOC inline block inside the
//! orchestrator, fanning out across macOS / iOS / tvOS / visionOS / watchOS /
//! Android / Linux / Windows / cross-compile permutations. Co-locating it here
//! lets the orchestrator stay focused on parse / lower / codegen / cache / link
//! sequencing instead of churning the same file every time a platform-specific
//! link flag changes.
//!
//! The `dylib` link path stays inline in compile.rs because it returns early
//! with a `CompileResult`. Per-platform `.app` bundling and Android companion
//! `.so` copying also stay in compile.rs — they happen after the link
//! returns and need access to many post-link variables (`exe_path`,
//! `result_bundle_id`, etc.) that don't belong in this module.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OutputFormat;

use super::{
    apple_sdk_version, build_geisterhand_libs, find_geisterhand_library,
    find_geisterhand_runtime, find_geisterhand_ui, find_lld_link, find_llvm_tool,
    find_msvc_lib_paths, find_msvc_link_exe, find_perry_windows_sdk,
    find_stdlib_library, find_ui_library, find_visionos_swift_runtime,
    find_watchos_swift_runtime, rust_target_triple, strip_duplicate_objects_from_lib,
    windows_pe_subsystem_flag, CompilationContext,
};

/// Construct the platform-specific linker command, append every required
/// argument (object files, libraries, frameworks, system libs, native libs,
/// geisterhand libs), invoke it, and bail on non-zero status.
///
/// Caller must have already handled the dylib output path; this function
/// only covers executable link. `args_input` is the user-supplied entry
/// `.ts` path (used for objcopy entry-stem matching on watchOS / visionOS /
/// iOS game-loop renames).
pub(super) fn build_and_run_link(
    args_input: &Path,
    ctx: &CompilationContext,
    target: Option<&str>,
    obj_paths: &[PathBuf],
    compiled_features: &[String],
    runtime_lib: &Path,
    stdlib_lib: &Option<PathBuf>,
    jsruntime_lib: &Option<PathBuf>,
    exe_path: &Path,
    format: OutputFormat,
) -> Result<()> {
    let is_ios = matches!(target, Some("ios-simulator") | Some("ios"));
    let is_visionos = matches!(target, Some("visionos-simulator") | Some("visionos"));
    let is_android = matches!(target, Some("android"));
    let is_harmonyos = matches!(target, Some("harmonyos") | Some("harmonyos-simulator"));
    let is_linux = matches!(target, Some("linux"))
        || (target.is_none() && cfg!(target_os = "linux"));
    let is_windows = matches!(target, Some("windows"))
        || (target.is_none() && cfg!(target_os = "windows"));
    let is_cross_windows = is_windows && !cfg!(target_os = "windows");
    let is_cross_ios = is_ios && !cfg!(target_os = "macos");
    let is_cross_visionos = is_visionos && !cfg!(target_os = "macos");
    let is_cross_macos = matches!(target, Some("macos")) && !cfg!(target_os = "macos");
    let is_watchos = matches!(target, Some("watchos") | Some("watchos-simulator"));
    let is_tvos = matches!(target, Some("tvos") | Some("tvos-simulator"));
    let is_cross_tvos = is_tvos && !cfg!(target_os = "macos");

    // For cross-compilation targets, use the appropriate toolchain
    let mut cmd = if is_watchos {
        let is_watchos_game_loop = compiled_features.iter().any(|f| f == "watchos-game-loop");
        let is_watchos_swift_app = compiled_features.iter().any(|f| f == "watchos-swift-app");
        let sdk = if target == Some("watchos-simulator") { "watchsimulator" } else { "watchos" };
        let sysroot = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]).output()?.stdout
        )?.trim().to_string();
        let triple = if target == Some("watchos-simulator") {
            "arm64-apple-watchos10.0-simulator"
        } else {
            "arm64_32-apple-watchos10.0"
        };

        // Find the entry object whose stem matches the user's input file stem
        // (e.g. `test_ui_counter.ts` → `test_ui_counter_ts.o`). Three rename targets:
        //   - Default (SwiftUI-tree app shell): `_main → _perry_main_init`, so the
        //     Swift `@main struct PerryApp` entry wins and calls back into TS init.
        //   - `--features watchos-game-loop`: `_main → _perry_user_main`, so the
        //     Rust runtime's `main()` (watchos_game_loop.rs) takes over the process
        //     entry, spawns the user's TS on a background thread, and calls
        //     `WKApplicationMain` on the main thread for a Metal/wgpu surface.
        //   - `--features watchos-swift-app`: `_main → _perry_user_main`, so the
        //     native lib's own `@main struct App: App` is the process entry.
        //     It spawns TS on a background thread from its `init()`/`.task {}`.
        let input_stem = args_input.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| format!("{}_ts", s))
            .unwrap_or_else(|| "main_ts".to_string());
        if let Some(entry_obj) = obj_paths.iter().find(|f| {
            f.file_stem().and_then(|s| s.to_str())
                .map(|s| s == input_stem.as_str() || s.ends_with(&format!("_{}", input_stem)))
                .unwrap_or(false)
        }) {
            let objcopy = std::env::var("HOME").ok()
                .map(|h| PathBuf::from(h).join(".rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-objcopy"))
                .filter(|p| p.exists())
                .or_else(|| std::env::var("HOME").ok()
                    .map(|h| PathBuf::from(h).join(".rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-objcopy"))
                    .filter(|p| p.exists()))
                .unwrap_or_else(|| PathBuf::from("rust-objcopy"));
            let rename = if is_watchos_game_loop || is_watchos_swift_app {
                "_main=__perry_user_main"
            } else {
                "_main=_perry_main_init"
            };
            let _ = Command::new(&objcopy)
                .args(["--redefine-sym", rename])
                .arg(entry_obj)
                .status();
        }

        if is_watchos_game_loop {
            // Game-loop: no SwiftUI scene tree — the native lib owns a
            // CAMetalLayer-backed view and `perry-runtime/watchos-game-loop`
            // provides the C `main()`. Link with clang, not swiftc.
            let clang = String::from_utf8(
                Command::new("xcrun").args(["--sdk", sdk, "--find", "clang"]).output()?.stdout
            )?.trim().to_string();
            let mut c = Command::new(clang);
            c.arg("-target").arg(triple)
             .arg("-isysroot").arg(&sysroot);
            c
        } else if is_watchos_swift_app {
            // Swift-app: the native lib ships its own `@main struct App: App`
            // (compiled separately in the native-lib loop below). Perry does
            // not emit PerryWatchApp.swift and does not provide a C main.
            // Use swiftc as the linker so Swift stdlib auto-links.
            let swiftc = String::from_utf8(
                Command::new("xcrun").args(["--sdk", sdk, "--find", "swiftc"]).output()?.stdout
            )?.trim().to_string();
            let mut c = Command::new(swiftc);
            c.arg("-target").arg(triple)
             .arg("-sdk").arg(&sysroot)
             .arg("-parse-as-library")
             // perry-runtime and the native lib each pull in their own std
             // rlibs (Cargo's metadata hashing differs across workspaces even
             // when -Zbuild-std flags match). Tell ld to take first-wins on
             // duplicates rather than fail the link.
             .arg("-Xlinker").arg("-ld_classic");
            c
        } else {
            let swiftc = String::from_utf8(
                Command::new("xcrun").args(["--sdk", sdk, "--find", "swiftc"]).output()?.stdout
            )?.trim().to_string();
            let swift_runtime = find_watchos_swift_runtime()
                .ok_or_else(|| anyhow!(
                    "PerryWatchApp.swift not found. Expected next to perry binary or in source tree."
                ))?;
            let mut c = Command::new(swiftc);
            c.arg("-target").arg(triple)
             .arg("-sdk").arg(&sysroot)
             .arg("-parse-as-library")
             .arg(&swift_runtime);
            c
        }
    } else if is_visionos && is_cross_visionos {
        return Err(anyhow!(
            "Local visionOS compilation requires Xcode on macOS. Use a macOS host or Perry Hub remote build."
        ));
    } else if is_visionos {
        let sdk = if target == Some("visionos-simulator") { "xrsimulator" } else { "xros" };
        let swiftc = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--find", "swiftc"]).output()?.stdout
        )?.trim().to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]).output()?.stdout
        )?.trim().to_string();
        let sdk_version = apple_sdk_version(sdk).unwrap_or_else(|| "1.0".to_string());
        let triple = if target == Some("visionos-simulator") {
            format!("arm64-apple-xros{}-simulator", sdk_version)
        } else {
            format!("arm64-apple-xros{}", sdk_version)
        };
        let swift_runtime = find_visionos_swift_runtime()
            .ok_or_else(|| anyhow!(
                "PerryVisionApp.swift not found. Expected next to perry binary or in source tree."
            ))?;

        let input_stem = args_input.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| format!("{}_ts", s))
            .unwrap_or_else(|| "main_ts".to_string());
        if let Some(entry_obj) = obj_paths.iter().find(|f| {
            f.file_stem().and_then(|s| s.to_str())
                .map(|s| s == input_stem.as_str() || s.ends_with(&format!("_{}", input_stem)))
                .unwrap_or(false)
        }) {
            let objcopy = std::env::var("HOME").ok()
                .map(|h| PathBuf::from(h).join(".rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-objcopy"))
                .filter(|p| p.exists())
                .or_else(|| std::env::var("HOME").ok()
                    .map(|h| PathBuf::from(h).join(".rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-objcopy"))
                    .filter(|p| p.exists()))
                .unwrap_or_else(|| PathBuf::from("rust-objcopy"));
            let _ = Command::new(&objcopy)
                .args(["--redefine-sym", "_main=_perry_main_init"])
                .arg(entry_obj)
                .status();
        }

        let mut c = Command::new(swiftc);
        c.arg("-target").arg(&triple)
         .arg("-sdk").arg(&sysroot)
         .arg("-parse-as-library")
         .arg(&swift_runtime);
        c
    } else if is_ios && is_cross_ios {
        // Cross-compile iOS from Linux using ld64.lld + Apple SDK sysroot
        let ld64 = find_llvm_tool("ld64.lld")
            .or_else(|| {
                // Check common paths
                for p in &["/usr/local/bin/ld64.lld", "/usr/bin/ld64.lld-18", "/usr/bin/ld64.lld"] {
                    if std::path::Path::new(p).exists() { return Some(PathBuf::from(p)); }
                }
                None
            })
            .unwrap_or_else(|| {
                eprintln!("Warning: ld64.lld not found for iOS cross-compilation. Install lld.");
                PathBuf::from("ld64.lld")
            });
        let sysroot = std::env::var("PERRY_IOS_SYSROOT")
            .unwrap_or_else(|_| "/opt/apple-sysroot/ios".to_string());
        eprintln!("[cross-ios] Using ld64.lld: {}", ld64.display());
        eprintln!("[cross-ios] Sysroot: {sysroot}");

        let mut c = Command::new(&ld64);
        c.arg("-arch").arg("arm64")
         .arg("-platform_version").arg("ios").arg("17.0.0").arg("26.0.0")
         .arg("-syslibroot").arg(&sysroot)
         .arg("-L").arg(format!("{}/usr/lib", sysroot))
         .arg("-L").arg(format!("{}/usr/lib/swift", sysroot))
         .arg("-F").arg(format!("{}/System/Library/Frameworks", sysroot))
         .arg("-lSystem")
         .arg("-dead_strip");
        c
    } else if is_ios {
        let sdk = if target == Some("ios-simulator") { "iphonesimulator" } else { "iphoneos" };
        let clang = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--find", "clang"]).output()?.stdout
        )?.trim().to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]).output()?.stdout
        )?.trim().to_string();
        let triple = if target == Some("ios-simulator") {
            "arm64-apple-ios17.0-simulator"
        } else {
            "arm64-apple-ios17.0"
        };

        // Discover Xcode developer directory for Swift standard library paths.
        // Swift libs live in the toolchain, not the SDK sysroot, so the linker
        // needs explicit -L flags to resolve auto-linked libs like swiftCore.
        let developer_dir = String::from_utf8(
            Command::new("xcode-select").arg("-p").output()?.stdout
        )?.trim().to_string();

        let mut c = Command::new(clang);
        c.arg("-target").arg(triple)
         .arg("-isysroot").arg(&sysroot)
         // Swift standard library .tbd stubs in the SDK (swiftCore, swift_Concurrency, etc.)
         .arg("-L").arg(format!("{}/usr/lib/swift", sysroot))
         // Swift compatibility static archives in the toolchain
         .arg("-L").arg(format!("{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/{}", developer_dir, sdk));
        c
    } else if is_tvos && is_cross_tvos {
        // Cross-compile tvOS from Linux using ld64.lld + Apple SDK sysroot.
        // The Linux builder worker ships a sysroot at /opt/apple-sysroot/tvos
        // (symlinked to the iOS sysroot — tvOS headers/libs are compatible with
        // the iOS SDK on aarch64 for our usage).
        let ld64 = find_llvm_tool("ld64.lld")
            .or_else(|| {
                // Check common paths
                for p in &["/usr/local/bin/ld64.lld", "/usr/bin/ld64.lld-18", "/usr/bin/ld64.lld"] {
                    if std::path::Path::new(p).exists() { return Some(PathBuf::from(p)); }
                }
                None
            })
            .unwrap_or_else(|| {
                eprintln!("Warning: ld64.lld not found for tvOS cross-compilation. Install lld.");
                PathBuf::from("ld64.lld")
            });
        let sysroot = std::env::var("PERRY_TVOS_SYSROOT")
            .unwrap_or_else(|_| "/opt/apple-sysroot/tvos".to_string());
        eprintln!("[cross-tvos] Using ld64.lld: {}", ld64.display());
        eprintln!("[cross-tvos] Sysroot: {sysroot}");

        // tvOS 17.0 minimum matches the non-cross branch's arm64-apple-tvos17.0 triple.
        // SDK version 26.0.0 matches the iOS cross branch (same Apple SDK release train).
        // Simulator (tvos-simulator) is not supported in the cross-compile path —
        // ld64.lld on Linux targets the device (arm64) only, matching is_cross_ios.
        let mut c = Command::new(&ld64);
        c.arg("-arch").arg("arm64")
         .arg("-platform_version").arg("tvos").arg("17.0.0").arg("26.0.0")
         .arg("-syslibroot").arg(&sysroot)
         .arg("-L").arg(format!("{}/usr/lib", sysroot))
         .arg("-L").arg(format!("{}/usr/lib/swift", sysroot))
         .arg("-F").arg(format!("{}/System/Library/Frameworks", sysroot))
         .arg("-lSystem")
         .arg("-dead_strip");
        c
    } else if is_tvos {
        let sdk = if target == Some("tvos-simulator") { "appletvsimulator" } else { "appletvos" };
        let clang = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--find", "clang"]).output()?.stdout
        )?.trim().to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun").args(["--sdk", sdk, "--show-sdk-path"]).output()?.stdout
        )?.trim().to_string();
        let triple = if target == Some("tvos-simulator") {
            "arm64-apple-tvos17.0-simulator"
        } else {
            "arm64-apple-tvos17.0"
        };

        let developer_dir = String::from_utf8(
            Command::new("xcode-select").arg("-p").output()?.stdout
        )?.trim().to_string();

        let mut c = Command::new(clang);
        c.arg("-target").arg(triple)
         .arg("-isysroot").arg(&sysroot)
         .arg("-L").arg(format!("{}/usr/lib/swift", sysroot))
         .arg("-L").arg(format!("{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/{}", developer_dir, sdk));
        c
    } else if is_android {
        // Use Android NDK clang to produce a shared library (.so)
        let ndk_home = std::env::var("ANDROID_NDK_HOME").map_err(|_| {
            anyhow!("ANDROID_NDK_HOME not set. Set it to your NDK path, e.g. $HOME/Library/Android/sdk/ndk/28.0.12433566")
        })?;
        let host_tag = if cfg!(target_os = "macos") { "darwin-x86_64" } else { "linux-x86_64" };
        let clang = format!(
            "{}/toolchains/llvm/prebuilt/{}/bin/aarch64-linux-android24-clang",
            ndk_home, host_tag
        );
        if !PathBuf::from(&clang).exists() {
            return Err(anyhow!("Android NDK clang not found at: {}", clang));
        }
        let mut c = Command::new(clang);
        c.arg("-shared")
         .arg("-fPIC")
         .arg("-target").arg("aarch64-linux-android24")
         .arg("-Wl,-z,max-page-size=16384")
         .arg("-Wl,-z,separate-loadable-segments")
         // Prevent ELF symbol interposition: bind all symbols within the .so
         // to the .so's own definitions. Without this, PLT calls (e.g. to "main")
         // can resolve to symbols from the host process (app_process/zygote),
         // bypassing perry's module initialization chain.
         .arg("-Wl,-Bsymbolic")
         // Allow unresolved symbols from namespace imports (import * as X).
         // The codegen emits short-name extern refs (__export_X) for namespace
         // imports that may not have a corresponding definition when the module
         // only exports individually-scoped symbols.
         .arg("-Wl,--warn-unresolved-symbols");
        c
    } else if is_harmonyos {
        // HarmonyOS NEXT: produce a musl-based ELF .so loaded by ArkTS via
        // napi_module_register (the NAPI wrapper lands in PR B.2). Uses the
        // OHOS SDK's clang from DevEco Studio; `--sysroot` + `-D__MUSL__`
        // match Huawei's hvigor-cc-invocation conventions.
        let sdk = super::library_search::find_harmonyos_sdk().ok_or_else(|| anyhow!(
            "OHOS SDK not found. Install DevEco Studio or the standalone \
             OpenHarmony SDK from https://developer.huawei.com/consumer/en/develop \
             and set OHOS_SDK_HOME to the SDK root (the dir that contains \
             native/llvm/bin/clang and native/sysroot/)."
        ))?;
        let clang = sdk.join("llvm").join("bin").join("clang");
        if !clang.exists() {
            return Err(anyhow!("OHOS SDK clang not found at: {}", clang.display()));
        }
        let clang_target = if target == Some("harmonyos-simulator") {
            "x86_64-linux-ohos"
        } else {
            "aarch64-linux-ohos"
        };
        let mut c = Command::new(clang);
        c.arg("-shared")
         .arg("-fPIC")
         .arg(format!("--target={}", clang_target))
         .arg(format!("--sysroot={}", sdk.join("sysroot").display()))
         .arg("-D__MUSL__")
         // Same interposition rationale as the Android branch — ArkTS loads
         // the .so into a host process that may expose its own `main`/malloc.
         .arg("-Wl,-Bsymbolic")
         .arg("-Wl,--warn-unresolved-symbols");
        c
    } else if is_linux {
        // Linux target: when running on Linux natively, just use "cc".
        // When cross-compiling from macOS, pass -target for clang.
        let mut c = Command::new("cc");
        #[cfg(not(target_os = "linux"))]
        {
            c.arg("-target").arg("x86_64-unknown-linux-gnu");
        }
        // Unresolved symbols are now link errors (not warnings). The
        // v0.5.0→0.5.18 Fastify/MySQL segfault (#28) was caused by
        // --warn-unresolved-symbols silently producing binaries with
        // null function pointers that crashed at runtime. With the
        // native module dispatch table restored, all expected symbols
        // are resolved; any remaining unresolved symbol is a real bug
        // that should fail the link rather than produce a broken binary.
        c
    } else if is_windows {
        // Windows target — two linker paths supported:
        //   Lightweight: lld-link (from LLVM) + xwin'd sysroot (from `perry setup windows`)
        //   MSVC:        link.exe + Visual Studio's VCTools + Windows SDK
        //
        // Precedence on native Windows:
        //   1. PERRY_LLD_LINK env var (explicit override — always wins)
        //   2. xwin'd sysroot present at %LOCALAPPDATA%\perry\windows-sdk → lld-link
        //      (if user ran `perry setup windows`, they've opted into this path)
        //   3. vswhere finds VCTools-enabled VS install → MSVC link.exe
        //   4. Bail with two-option install hint
        let linker = if let Ok(lld) = std::env::var("PERRY_LLD_LINK") {
            PathBuf::from(lld)
        } else if !is_cross_windows && find_perry_windows_sdk().is_some() {
            // User ran `perry setup windows`. Use LLVM's lld-link.
            match find_lld_link() {
                Some(p) => p,
                None => {
                    return Err(anyhow!(
                        "`perry setup windows` has populated a Windows SDK at {} but \
                         LLVM's lld-link.exe is missing. Install LLVM via:\n\
                         \x20  winget install LLVM.LLVM\n\
                         then open a new terminal and retry.",
                        find_perry_windows_sdk().unwrap().display()
                    ));
                }
            }
        } else if let Some(path) = find_msvc_link_exe() {
            path
        } else if is_cross_windows {
            eprintln!("Warning: lld-link not found for cross-compilation. Install: rustup component add llvm-tools");
            PathBuf::from("link.exe")
        } else {
            // Native Windows: neither MSVC (via vswhere) nor the xwin'd sysroot
            // is present. Fail fast with both install paths — matches the
            // `find_clang` context pattern in perry-codegen/src/linker.rs.
            return Err(anyhow!(
                "No Windows linker toolchain found. Perry needs either MSVC link.exe + \
                 Windows SDK, or LLVM's lld-link + the xwin'd sysroot from `perry setup \
                 windows`. Pick whichever is lighter for you:\n\
                 \n\
                 \x20  A) Lightweight (LLVM + xwin, ~1.5 GB, no Visual Studio needed):\n\
                 \x20       winget install LLVM.LLVM\n\
                 \x20       perry setup windows\n\
                 \n\
                 \x20  B) MSVC (Visual Studio Build Tools + C++ workload, ~8 GB):\n\
                 \x20       Visual Studio Installer → Modify → \"Desktop development with C++\"\n\
                 \x20       or: winget install Microsoft.VisualStudio.2022.BuildTools --override \
                 \"--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended\"\n\
                 \n\
                 Then open a new terminal and retry. Run `perry doctor` to verify."
            ));
        };
        let mut c = Command::new(linker);
        // /ENTRY:mainCRTStartup works for both subsystems: Perry emits
        // `int main()` and the MSVC CRT invokes it regardless of subsystem.
        // See windows_pe_subsystem_flag() for subsystem selection rationale.
        c.arg(windows_pe_subsystem_flag(ctx.needs_ui))
         .arg("/ENTRY:mainCRTStartup")
         .arg("/NOLOGO")
         // Perry generates large init functions for TS modules (one function
         // per module). Large codebases (100+ modules) can overflow the
         // default 1MB stack. Reserve 8MB.
         .arg("/STACK:67108864")
         // Native libs (hone_editor_windows etc) bundle perry_runtime objects
         // that can't be fully stripped. Identical symbols are safe to merge.
         .arg("/FORCE:MULTIPLE");
        // Set up MSVC library search paths if LIB env isn't already configured
        if std::env::var("LIB").is_err() {
            if let Some(lib_paths) = find_msvc_lib_paths() {
                c.env("LIB", lib_paths);
            } else if is_cross_windows {
                eprintln!("Warning: No Windows SDK library paths found. Set PERRY_WINDOWS_SYSROOT to your xwin sysroot.");
            }
        }
        c
    } else if is_cross_macos {
        // Cross-compile macOS from Linux using ld64.lld + Apple SDK sysroot
        let ld64 = find_llvm_tool("ld64.lld")
            .or_else(|| {
                for p in &["/usr/local/bin/ld64.lld", "/usr/bin/ld64.lld-18", "/usr/bin/ld64.lld"] {
                    if std::path::Path::new(p).exists() { return Some(PathBuf::from(p)); }
                }
                None
            })
            .unwrap_or_else(|| {
                eprintln!("Warning: ld64.lld not found for macOS cross-compilation. Install lld.");
                PathBuf::from("ld64.lld")
            });
        let sysroot = std::env::var("PERRY_MACOS_SYSROOT")
            .unwrap_or_else(|_| "/opt/apple-sysroot/macos".to_string());
        eprintln!("[cross-macos] Using ld64.lld: {}", ld64.display());
        eprintln!("[cross-macos] Sysroot: {sysroot}");

        let mut c = Command::new(&ld64);
        c.arg("-arch").arg("arm64")
         .arg("-platform_version").arg("macos").arg("13.0.0").arg("26.0.0")
         .arg("-syslibroot").arg(&sysroot)
         .arg("-L").arg(format!("{}/usr/lib", sysroot))
         .arg("-L").arg(format!("{}/usr/lib/swift", sysroot))
         .arg("-F").arg(format!("{}/System/Library/Frameworks", sysroot))
         .arg("-lSystem")
         .arg("-dead_strip");
        c
    } else {
        Command::new("cc")
    };

    // When ios-game-loop is enabled, rename _main to _perry_user_main in the
    // entry object file so the perry runtime's main() (from ios_game_loop.rs)
    // becomes the process entry point. It spawns _perry_user_main on a game thread.
    if (is_ios || is_tvos) && compiled_features.iter().any(|f| f == "ios-game-loop") {
        if let Some(entry_obj) = obj_paths.iter().find(|f| f.to_string_lossy().contains("main_ts")) {
            // Try rust-objcopy first (newer Rust), then llvm-objcopy (older Rust)
            let objcopy = std::env::var("HOME").ok()
                .map(|h| PathBuf::from(h).join(".rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-objcopy"))
                .filter(|p| p.exists())
                .or_else(|| std::env::var("HOME").ok()
                    .map(|h| PathBuf::from(h).join(".rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/llvm-objcopy"))
                    .filter(|p| p.exists()))
                .unwrap_or_else(|| PathBuf::from("rust-objcopy"));
            let _ = Command::new(&objcopy)
                .args(["--redefine-sym", "_main=__perry_user_main"])
                .arg(entry_obj)
                .status();
        }
    }

    for obj_path in obj_paths {
        cmd.arg(obj_path);
    }

    // HarmonyOS: pick up native C objects that build.rs scripts emitted
    // alongside the Rust artifacts. Rust's staticlib normally bundles these
    // into libperry_runtime.a, but on our macOS→OHOS cross-build the
    // `libmimalloc.a` wrapper ends up as a zero-member BSD-format archive
    // (BSD ar's `__.SYMDEF SORTED` layout — macOS-host `ar` creates it,
    // llvm-ar can't read it back), and rustc's "bundle native libs into
    // the staticlib" path silently skips it. Without us forwarding the
    // loose .o files to the final link, `libentry.so` ends up with
    // `mi_malloc_aligned` marked UND, and the OHOS dynamic linker rejects
    // dlopen with "symbol not found" at EntryAbility.onCreate time.
    //
    // We walk `target/<triple>/release/build/*/out/` and collect every
    // loose .o. This is coarser than Rust's per-crate link-lib directive
    // walking — it picks up .o files from any transitive C dep, not just
    // mimalloc — but that's a feature: the set is tiny in practice
    // (mimalloc is the only C dep in perry-runtime's closure today) and
    // any that turn out unreferenced are dead-stripped via --gc-sections.
    if is_harmonyos {
        let triple = super::rust_target_triple(target).unwrap_or("aarch64-unknown-linux-ohos");
        let build_roots: Vec<std::path::PathBuf> = {
            let mut roots: Vec<std::path::PathBuf> = Vec::new();
            // auto_rebuild emits into a perry-auto-<hash> dir; the workspace's
            // own target/ is a fallback for non-auto flows.
            if let Ok(entries) = std::fs::read_dir("target") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("perry-auto-") || name_str == triple {
                        roots.push(entry.path());
                    }
                }
            }
            // When invoked from outside the workspace, auto_rebuild still
            // lands under the perry source tree's target/. Add that.
            if let Some(ws_root) = super::find_perry_workspace_root() {
                let ws_target = ws_root.join("target");
                if let Ok(entries) = std::fs::read_dir(&ws_target) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if name_str.starts_with("perry-auto-") {
                            roots.push(entry.path());
                        }
                    }
                }
            }
            roots
        };
        let mut native_objs: Vec<std::path::PathBuf> = Vec::new();
        for root in &build_roots {
            let build_dir = root.join(triple).join("release").join("build");
            let entries = match std::fs::read_dir(&build_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for crate_build in entries.flatten() {
                let out_dir = crate_build.path().join("out");
                // Walk the out/ dir recursively (cc-rs can nest into source-
                // mirror subdirs like c_src/mimalloc/v2/src/).
                if let Ok(walker) = walkdir::WalkDir::new(&out_dir).into_iter().collect::<Result<Vec<_>, _>>() {
                    for entry in walker {
                        if entry.file_type().is_file()
                            && entry.path().extension().and_then(|e| e.to_str()) == Some("o")
                        {
                            native_objs.push(entry.path().to_path_buf());
                        }
                    }
                }
            }
        }
        if !native_objs.is_empty() && matches!(format, crate::OutputFormat::Text) {
            println!("  harmonyos: linking {} build.rs native object(s)", native_objs.len());
        }
        for obj in native_objs {
            cmd.arg(obj);
        }
    }

    // Dead code stripping — safe because compile_init() emits func_addr
    // calls for every class method/getter during vtable registration. These
    // serve as linker roots that keep dynamically-dispatched methods alive.
    if !is_windows {
        if is_android || is_linux || is_harmonyos {
            cmd.arg("-Wl,--gc-sections");
        } else if is_cross_ios || is_cross_visionos || is_cross_macos || is_cross_tvos {
            // ld64.lld called directly — no -Wl, prefix needed
            cmd.arg("-dead_strip");
        } else if is_watchos || is_visionos {
            cmd.arg("-Xlinker").arg("-dead_strip");
        } else {
            // Native macOS/iOS via clang driver
            cmd.arg("-Wl,-dead_strip");
        }
    } else {
        // MSVC link.exe / lld-link equivalents:
        //   /OPT:REF — drop unreferenced functions/data (= --gc-sections)
        //   /OPT:ICF — fold identical COMDATs (= --icf=safe)
        // These are documented as defaults under /RELEASE, but Perry doesn't
        // pass /RELEASE so the linker falls back to /OPT:NOREF, pulling in the
        // entire perry-stdlib archive even when only a fraction is used.
        cmd.arg("/OPT:REF").arg("/OPT:ICF");
    }

    // Link libraries - jsruntime bundles V8 + stdlib; runtime provides base FFI symbols.
    // Note: libperry_jsruntime.a omits some runtime symbols (js_register_class_method,
    // js_register_class_getter, etc.) due to Rust DCE on rlib dependencies. We always
    // link libperry_runtime.a as a fallback to fill these gaps. On macOS/Linux/ELF the
    // linker uses first-definition-wins for archives, so no duplicate symbol errors arise.
    // When UI lib is also linked, it bundles its own copy of perry-runtime.
    // For Android (ELF), skip the extra runtime when UI provides it.
    // On Windows (MSVC), always link the runtime — the UI lib's rlib dependency on
    // perry-runtime may not include all symbols (e.g., perry_init_guard_check_and_set).
    // watchOS: swiftc treats duplicate symbols as errors (not warnings like clang),
    // so skip the standalone runtime when the UI lib already bundles it.
    // Note: even when bitcode_linked is true, we still link the .a archives.
    // The merged .o contains the crate code but NOT the Rust standard library
    // symbols (alloc, std::thread_local, etc.). The .a archive provides those
    // as a fallback — the linker only pulls object files from the .a that
    // resolve still-undefined symbols (first-definition-wins on macOS).
    let skip_runtime = (is_android || is_watchos || is_visionos)
        && ctx.needs_ui
        && find_ui_library(target).is_some();
    if !skip_runtime {
        if let Some(ref jsruntime) = jsruntime_lib {
            cmd.arg(jsruntime);
            // Also link runtime to supply symbols DCE'd from jsruntime (e.g. js_register_class_method)
            if !is_android && !is_windows {
                cmd.arg(runtime_lib);
            }
        } else if ctx.needs_stdlib || is_windows {
            // On Windows/MSVC, always try to link stdlib because codegen unconditionally
            // declares all stdlib extern functions, creating import references that MSVC
            // won't dead-strip. On macOS/Linux, the linker ignores unreferenced archives.
            if let Some(ref stdlib) = stdlib_lib {
                cmd.arg(stdlib);
                // Also link runtime to supply symbols that may be DCE'd from stdlib's
                // bundled perry-runtime (e.g. js_closure_unbind_this, js_string_addref)
                if !is_android && !is_windows {
                    cmd.arg(runtime_lib);
                }
            } else {
                if ctx.needs_stdlib {
                    eprintln!("Warning: stdlib required but {} not found, using runtime-only",
                        if is_windows { "perry_stdlib.lib" } else { "libperry_stdlib.a" });
                }
                cmd.arg(runtime_lib);
            }
        } else {
            // Runtime-only linking — no stdlib needed
            cmd.arg(runtime_lib);
        }
    } else if ctx.needs_stdlib {
        // Android + UI: runtime is provided by UI lib, but stdlib must still be linked
        // separately (UI lib does not bundle perry-stdlib).
        if let Some(ref stdlib) = stdlib_lib {
            cmd.arg(stdlib);
        } else {
            eprintln!("Warning: stdlib required but libperry_stdlib.a not found");
        }
    }

    if is_windows {
        cmd.arg(format!("/OUT:{}", exe_path.display()));
        // V8/deno_core needs additional Windows system libraries
        if jsruntime_lib.is_some() {
            cmd.arg("winmm.lib");
            cmd.arg("dbghelp.lib");
            cmd.arg("msvcprt.lib"); // C++ runtime for exception_ptr
        }
    } else {
        cmd.arg("-o")
            .arg(exe_path)
            .arg("-lc");
    }

    // For plugin hosts, export symbols so dlopen'd plugins can resolve them.
    // Plugins are dylibs loaded via dlopen — they need to resolve:
    //   1. hone_host_api_* (plugin→host calls)
    //   2. js_*/perry_* (Perry runtime used by compiled plugin code)
    // We use -u to prevent dead_strip from removing these, keeping binary size small.
    if ctx.needs_plugins && !is_windows {
        #[cfg(target_os = "macos")]
        {
            // Force-keep all functions from plugin-related native libraries
            for native_lib in &ctx.native_libraries {
                if native_lib.module.contains("plugin") {
                    for func in &native_lib.functions {
                        cmd.arg(format!("-Wl,-u,_{}", func.name));
                    }
                }
            }
            // Force-keep Perry runtime symbols that plugin dylibs reference.
            // These are collected from the Perry runtime's public API.
            // Using -u tells the linker "treat as referenced" so dead_strip keeps them.
            let runtime_syms = [
                "js_array_alloc",
                "js_array_from_f64", "js_array_push_f64",
                "js_bigint_is_zero",
                "js_closure_alloc",
                "js_console_log_spread",
                "js_dynamic_object_get_property",
                "js_dynamic_string_equals",
                "js_gc_register_global_root",
                "js_is_truthy",
                "js_jsvalue_compare", "js_jsvalue_equals",
                "js_nanbox_get_pointer", "js_nanbox_pointer", "js_nanbox_string",
                "js_native_call_method",
                "js_object_alloc_class_with_keys", "js_object_alloc_with_shape",
                "js_register_class_method",
                "js_string_char_code_at", "js_string_from_bytes", "js_string_length",
                "perry_debug_trace_init", "perry_debug_trace_init_done",
                "perry_init_guard_check_and_set",
            ];
            for sym in &runtime_syms {
                cmd.arg(format!("-Wl,-u,_{}", sym));
            }
        }
        #[cfg(target_os = "linux")]
        {
            cmd.arg("-rdynamic");
        }
    }

    if is_watchos {
        // watchOS frameworks (swiftc auto-links Swift stdlib on the non-game-loop path)
        let is_watchos_game_loop = compiled_features.iter().any(|f| f == "watchos-game-loop");
        let is_watchos_swift_app = compiled_features.iter().any(|f| f == "watchos-swift-app");
        if !is_watchos_game_loop {
            cmd.arg("-framework").arg("SwiftUI");
        }
        cmd.arg("-framework").arg("WatchKit")
           .arg("-framework").arg("Foundation")
           .arg("-framework").arg("CoreFoundation")
           .arg("-framework").arg("Security")
           .arg("-lSystem")
           .arg("-lresolv");
        if is_watchos_game_loop {
            // QuartzCore for CAMetalLayer-backed rendering (Metal.framework is NOT
            // in the watchOS SDK — the native lib must dlopen it or supply its own
            // path to the device's Metal dylib). -lobjc for the dynamic
            // WKApplicationDelegate class registered from watchos_game_loop.rs.
            cmd.arg("-framework").arg("QuartzCore")
               .arg("-lobjc");
        }
        if is_watchos_swift_app {
            // SceneKit for SceneView-backed 3D rendering from the native lib's
            // `@main struct App: App`. The lib may additionally use Canvas (2D,
            // already covered by SwiftUI) or SpriteKit (opt-in via the
            // manifest's `frameworks` list).
            cmd.arg("-framework").arg("SceneKit");
        }
    } else if is_ios {
        // iOS frameworks
        cmd.arg("-framework").arg("UIKit")
           .arg("-framework").arg("Foundation")
           .arg("-framework").arg("CoreGraphics")
           .arg("-framework").arg("Security")
           .arg("-framework").arg("CoreFoundation")
           .arg("-framework").arg("SystemConfiguration")
           .arg("-framework").arg("QuartzCore")
           .arg("-framework").arg("AVFAudio") // AVAudioEngine for audio capture
           .arg("-framework").arg("AVFoundation") // Camera capture (AVCaptureSession)
           .arg("-framework").arg("CoreMedia") // CMSampleBuffer
           .arg("-framework").arg("CoreVideo") // CVPixelBuffer
           .arg("-framework").arg("UserNotifications") // UNUserNotificationCenter (perry/system notificationSend)
           .arg("-framework").arg("CoreLocation") // CLCircularRegion for UNLocationNotificationTrigger (#96)
           .arg("-liconv")
           .arg("-lresolv")
           .arg("-lobjc")
           .arg("-lSystem");
    } else if is_visionos {
        cmd.arg("-framework").arg("SwiftUI")
           .arg("-framework").arg("UIKit")
           .arg("-framework").arg("Foundation")
           .arg("-framework").arg("CoreGraphics")
           .arg("-framework").arg("Security")
           .arg("-framework").arg("CoreFoundation")
           .arg("-framework").arg("SystemConfiguration")
           .arg("-framework").arg("QuartzCore")
           .arg("-framework").arg("AVFAudio")
           .arg("-framework").arg("AVFoundation")
           .arg("-framework").arg("CoreMedia")
           .arg("-framework").arg("CoreVideo")
           .arg("-liconv")
           .arg("-lresolv")
           .arg("-lobjc")
           .arg("-lSystem");
    } else if is_tvos {
        // tvOS frameworks (UIKit-based, like iOS)
        cmd.arg("-framework").arg("UIKit")
           .arg("-framework").arg("Foundation")
           .arg("-framework").arg("CoreGraphics")
           .arg("-framework").arg("Security")
           .arg("-framework").arg("CoreFoundation")
           .arg("-framework").arg("SystemConfiguration")
           .arg("-framework").arg("QuartzCore")
           .arg("-framework").arg("AVFoundation")
           .arg("-framework").arg("GameController")
           .arg("-framework").arg("Metal")
           .arg("-liconv")
           .arg("-lresolv")
           .arg("-lobjc")
           .arg("-lSystem");
    } else if is_harmonyos {
        // OpenHarmony system libraries. musl folds m/pthread/dl into libc.a so
        // the -l flags are no-ops on the toolchain side; we emit them anyway
        // because cargo's static archives reference them and the OHOS dynamic
        // linker resolves them at load time.
        cmd.arg("-Wl,--allow-multiple-definition")
           .arg("-lm")
           .arg("-lpthread")
           .arg("-ldl");
        // `libace_napi.z.so` provides napi_module_register + napi_create_*
        // (consumed by perry-runtime/src/ohos_napi.rs). OHOS naming convention
        // is `<name>.z.so` — the `-l` flag strips `lib` and `.so` but NOT the
        // middle `.z`, so `-lace_napi.z` is the deliberate spelling.
        cmd.arg("-lace_napi.z");
    } else if is_android {
        // Android system libraries
        cmd.arg("-Wl,--allow-multiple-definition")
           .arg("-lm")
           .arg("-ldl")
           .arg("-llog");

        // Stub for JNI_GetCreatedJavaVMs: the jni-sys crate declares this extern
        // symbol, but Android has no libjvm.so and libnativehelper.so is only
        // available at API 31+. Perry gets the JavaVM from JNI_OnLoad and never
        // calls this function, so compile a no-op C stub to satisfy the linker.
        let stub_dir = std::env::temp_dir().join(format!("perry_jni_stub_{}", std::process::id()));
        std::fs::create_dir_all(&stub_dir).ok();
        let stub_c = stub_dir.join("jni_stub.c");
        let stub_o = stub_dir.join("jni_stub.o");
        std::fs::write(&stub_c, concat!(
            "typedef int jint;\n",
            "typedef jint jsize;\n",
            "jint JNI_GetCreatedJavaVMs(void **vm_buf, jsize buf_len, jsize *n_vms) {\n",
            "    if (n_vms) *n_vms = 0;\n",
            "    return 0;\n",
            "}\n",
        )).ok();
        let ndk_home = std::env::var("ANDROID_NDK_HOME").unwrap_or_default();
        let host_tag = if cfg!(target_os = "macos") { "darwin-x86_64" } else { "linux-x86_64" };
        let ndk_clang = format!(
            "{}/toolchains/llvm/prebuilt/{}/bin/aarch64-linux-android24-clang",
            ndk_home, host_tag
        );
        let stub_ok = Command::new(&ndk_clang)
            .args(["-c", "-fPIC", "-target", "aarch64-linux-android24"])
            .arg("-o").arg(&stub_o)
            .arg(&stub_c)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if stub_ok {
            cmd.arg(&stub_o);
        }
    } else if is_linux {
        // Linux system libraries (cross-compile target)
        // Allow multiple definitions: perry-jsruntime embeds perry-runtime symbols,
        // and we also link perry-runtime directly for symbols DCE'd from jsruntime.
        // macOS Mach-O uses first-definition-wins natively; ELF linkers need this flag.
        cmd.arg("-Wl,--allow-multiple-definition")
           .arg("-lm")
           .arg("-lpthread")
           .arg("-ldl");

        if ctx.needs_stdlib || jsruntime_lib.is_some() {
            cmd.arg("-lssl")
               .arg("-lcrypto");
        }

        if jsruntime_lib.is_some() {
            cmd.arg("-lstdc++");
        }
    } else if is_windows {
        // Windows system libraries
        cmd.arg("user32.lib")
           .arg("gdi32.lib")
           .arg("gdiplus.lib")
           .arg("msimg32.lib")
           .arg("kernel32.lib")
           .arg("shell32.lib")
           .arg("ole32.lib")
           .arg("comctl32.lib")
           .arg("advapi32.lib")
           .arg("comdlg32.lib")
           .arg("ws2_32.lib")
           .arg("dwmapi.lib");
        // MSVC CRT (dynamic) and additional Windows API libraries needed by the Rust runtime
        cmd.arg("msvcrt.lib")
           .arg("vcruntime.lib")
           .arg("ucrt.lib")
           .arg("bcrypt.lib")
           .arg("ntdll.lib")
           .arg("userenv.lib")
           // secur32.lib exports `GetUserNameExW`, called by the `whoami`
           // crate (transitively pulled in via `sqlx-mysql`/`sqlx-postgres`
           // through `perry-stdlib`). Without it, every doc-test that
           // touches stdlib fails on the Windows runner with
           // `LNK2019: unresolved external symbol __imp_GetUserNameExW`.
           // Closes #220.
           .arg("secur32.lib")
           .arg("oleaut32.lib")
           .arg("propsys.lib")
           .arg("runtimeobject.lib")
           .arg("iphlpapi.lib");
    } else {
        // macOS frameworks for runtime (sysinfo, etc.) and V8.
        // Gate on `!is_harmonyos` so the macOS host doesn't leak its
        // frameworks into ELF cross-compile targets that fall through this
        // `else` branch — `cfg!(target_os = "macos")` is true whenever we're
        // running ON macOS, regardless of the actual target.
        if (cfg!(target_os = "macos") || is_cross_macos) && !is_harmonyos {
            cmd.arg("-framework").arg("Security")
               .arg("-framework").arg("CoreFoundation")
               .arg("-framework").arg("SystemConfiguration")
               .arg("-liconv")
               .arg("-lresolv")
               .arg("-lobjc");

            if jsruntime_lib.is_some() {
                cmd.arg("-lc++");
            }
        }

        // On Linux (native, not cross-compiling to macOS), link against system libraries
        if cfg!(target_os = "linux") && !is_cross_macos {
            cmd.arg("-lm")
               .arg("-lpthread")
               .arg("-ldl");

            if ctx.needs_stdlib || jsruntime_lib.is_some() {
                cmd.arg("-lssl")
                   .arg("-lcrypto");
            }

            if jsruntime_lib.is_some() {
                cmd.arg("-lstdc++");
            }
        }
    }

    // Link perry/ui library and platform frameworks if needed
    if ctx.needs_ui {
        // When geisterhand is enabled, prefer the geisterhand-enabled UI lib
        // (it contains widget registration calls that the normal lib doesn't have)
        let ui_lib_option = if ctx.needs_geisterhand {
            find_geisterhand_ui(target).or_else(|| find_ui_library(target))
        } else {
            find_ui_library(target)
        };
        if let Some(ui_lib) = ui_lib_option {
            // The UI staticlib bundles perry_runtime + Rust std. When perry-stdlib
            // is also linked (which bundles the same), duplicate symbols cause
            // crashes (conflicting static state initialization). Strip duplicates
            // on Apple platforms. On Windows/Android, skip strip-dedup because
            // perry_runtime objects contain monomorphizations needed by UI code,
            // and --allow-multiple-definition (ELF) / /FORCE:MULTIPLE (COFF)
            // handles duplicate symbols safely. On Android, skip_runtime=true
            // means the UI lib is the sole provider of perry-runtime symbols.
            let ui_lib = if is_windows || is_android || is_visionos {
                ui_lib
            } else {
                match strip_duplicate_objects_from_lib(&ui_lib) {
                    Ok(trimmed) => trimmed,
                    Err(e) => {
                        eprintln!("[strip-dedup] skipped for UI lib (non-fatal): {e}");
                        ui_lib
                    }
                }
            };
            if is_windows {
                // lld-link scans archives left-to-right once. The UI lib is
                // linked before user code objects, so UI symbols aren't yet
                // undefined when the lib is scanned. /WHOLEARCHIVE forces all
                // objects from the archive to be included unconditionally.
                cmd.arg(format!("/WHOLEARCHIVE:{}", ui_lib.display()));
            } else {
                cmd.arg(&ui_lib);
            }

            if is_watchos {
                // SwiftUI/WatchKit already linked above
            } else if is_ios || is_visionos || is_tvos {
                // UIKit already linked above
            } else if is_android {
                // Allow multiple definitions from perry-runtime in both UI lib and native libs
                cmd.arg("-Wl,--allow-multiple-definition");
            } else if is_linux {
                // Allow multiple definitions from perry-runtime in both stdlib and UI lib
                cmd.arg("-Wl,--allow-multiple-definition");
                // libperry_ui_gtk4.a's glib::source::trampoline_local
                // closures call perry-stdlib's js_stdlib_process_pending /
                // js_promise_run_microtasks. When ctx.needs_stdlib is false
                // (bare UI program), stdlib isn't linked via the earlier
                // path. Force-link it here with --whole-archive so every
                // object is pulled unconditionally. --allow-multiple-definition
                // above lets it coexist with the runtime stub at
                // perry-runtime/src/stdlib_stubs.rs. The async-runtime
                // feature is force-enabled for UI builds (see
                // build_optimized_libs), so the real js_stdlib_process_pending
                // is guaranteed present in libperry_stdlib.a.
                let linux_stdlib_for_ui = stdlib_lib.clone()
                    .or_else(|| find_stdlib_library(target));
                if let Some(ref stdlib) = linux_stdlib_for_ui {
                    cmd.arg("-Wl,--whole-archive")
                       .arg(stdlib)
                       .arg("-Wl,--no-whole-archive");
                }
                // GTK4 libraries via pkg-config. The fallback fires in two
                // distinct cases: pkg-config not installed (spawn fails), OR
                // installed but `gtk4.pc` not on the search path (exit != 0
                // — happens e.g. on Ubuntu hosts where libgtk-4-dev is split
                // across packages, or when PKG_CONFIG_PATH is locked down).
                // Pre-fix the second case silently emitted no GTK link flags
                // and the link bombed with hundreds of `g_object_unref` /
                // `gtk_widget_*` undefined references (#181).
                let mut got_gtk_libs = false;
                let pc_out = Command::new("pkg-config").args(["--libs", "gtk4"]).output();
                if let Ok(ref output) = pc_out {
                    if output.status.success() {
                        let libs = String::from_utf8_lossy(&output.stdout);
                        for flag in libs.trim().split_whitespace() {
                            cmd.arg(flag);
                        }
                        got_gtk_libs = true;
                    }
                }
                if !got_gtk_libs {
                    // Mirrors what `pkg-config --libs gtk4` returns on a
                    // standard libgtk-4-dev install. Pre-fix only listed the
                    // glib/gio core, which left pango/cairo/gdk_pixbuf
                    // undefined.
                    eprintln!(
                        "Warning: `pkg-config --libs gtk4` did not return GTK4 \
                         linker flags ({}). Falling back to a hardcoded GTK4 \
                         link set — install `libgtk-4-dev` (Debian/Ubuntu) or \
                         `gtk4-devel` (Fedora/RHEL) and ensure pkg-config can \
                         find `gtk4.pc` to silence this warning.",
                        match &pc_out {
                            Err(e) => format!("pkg-config not runnable: {e}"),
                            Ok(o) if !o.status.success() => format!(
                                "pkg-config exited {}: {}",
                                o.status.code().unwrap_or(-1),
                                String::from_utf8_lossy(&o.stderr).trim()
                            ),
                            Ok(_) => "no output".to_string(),
                        }
                    );
                    for lib in [
                        "-lgtk-4", "-lgio-2.0", "-lgobject-2.0", "-lglib-2.0",
                        "-lpangocairo-1.0", "-lpango-1.0", "-lharfbuzz",
                        "-lgdk_pixbuf-2.0", "-lcairo-gobject", "-lcairo",
                        "-lgraphene-1.0",
                    ] {
                        cmd.arg(lib);
                    }
                }
                // PulseAudio for audio capture (only needed with UI)
                cmd.arg("-lpulse-simple")
                   .arg("-lpulse");
            } else if is_windows {
                // Win32 system libs already linked above
            } else {
                if cfg!(target_os = "macos") || is_cross_macos {
                    cmd.arg("-framework").arg("AppKit");
                    cmd.arg("-framework").arg("CoreGraphics");
                    cmd.arg("-framework").arg("QuartzCore");
                    cmd.arg("-framework").arg("AVFoundation");
                    cmd.arg("-framework").arg("Metal");
                    cmd.arg("-framework").arg("IOKit");
                    cmd.arg("-framework").arg("DiskArbitration"); // needed by CoreGraphics
                }
            }

            match format {
                OutputFormat::Text => println!("Linking perry/ui (native UI) from {}", ui_lib.display()),
                OutputFormat::Json => {}
            }
        } else {
            let (lib_name, build_cmd) = if is_watchos {
                ("libperry_ui_watchos.a", "cargo build --release -p perry-ui-watchos --target arm64_32-apple-watchos")
            } else if is_tvos {
                ("libperry_ui_tvos.a", "cargo build --release -p perry-ui-tvos --target aarch64-apple-tvos")
            } else if is_visionos {
                ("libperry_ui_visionos.a", "cargo build --release -p perry-ui-visionos --target aarch64-apple-visionos-sim")
            } else if is_ios {
                ("libperry_ui_ios.a", "cargo build --release -p perry-ui-ios --target aarch64-apple-ios-sim")
            } else if is_android {
                ("libperry_ui_android.a", "cargo build --release -p perry-ui-android --target aarch64-linux-android")
            } else if is_linux {
                ("libperry_ui_gtk4.a", "cargo build --release -p perry-ui-gtk4 --target x86_64-unknown-linux-gnu")
            } else if is_windows {
                ("perry_ui_windows.lib", "cargo build --release -p perry-ui-windows --target x86_64-pc-windows-msvc")
            } else {
                ("libperry_ui_macos.a", "cargo build --release -p perry-ui-macos")
            };
            return Err(anyhow!(
                "perry/ui imported but {} not found. Build with: {}", lib_name, build_cmd
            ));
        }
    }

    // Link geisterhand libraries if enabled
    if ctx.needs_geisterhand {
        // Auto-build geisterhand libraries if any are missing
        let gh_missing = find_geisterhand_library(target).is_none()
            || find_geisterhand_runtime(target).is_none()
            || (ctx.needs_ui && find_geisterhand_ui(target).is_none());
        if gh_missing {
            build_geisterhand_libs(target, format)?;
        }

        if let Some(gh_lib) = find_geisterhand_library(target) {
            cmd.arg(&gh_lib);
            // Link geisterhand-enabled runtime (has the registry + pump functions)
            if let Some(gh_runtime) = find_geisterhand_runtime(target) {
                cmd.arg(&gh_runtime);
                // ELF linkers need --allow-multiple-definition; macOS Mach-O uses first-wins natively
                if is_linux || is_android {
                    cmd.arg("-Wl,--allow-multiple-definition");
                }
            }
            // On Windows, re-link the stdlib after geisterhand to resolve
            // forward references to geisterhand registry functions.
            // lld-link scans archives left-to-right once, so the stdlib
            // must appear after the geisterhand lib that references it.
            // On Windows, force-include geisterhand registry symbols from stdlib.
            // lld-link scans archives left-to-right once, so the stdlib's
            // geisterhand objects are skipped on first scan (no references yet).
            // /INCLUDE forces the linker to pull in the specific symbols.
            if is_windows {
                cmd.arg("/INCLUDE:perry_geisterhand_queue_action");
                cmd.arg("/INCLUDE:perry_geisterhand_queue_action1");
                cmd.arg("/INCLUDE:perry_geisterhand_queue_state_set");
                cmd.arg("/INCLUDE:perry_geisterhand_request_screenshot");
                cmd.arg("/INCLUDE:perry_geisterhand_register");
                cmd.arg("/INCLUDE:perry_geisterhand_pump");
                cmd.arg("/INCLUDE:perry_geisterhand_start");
                cmd.arg("/INCLUDE:perry_geisterhand_free_string");
                cmd.arg("/INCLUDE:perry_geisterhand_get_closure");
                cmd.arg("/INCLUDE:perry_geisterhand_get_registry_json");
                // Allow duplicate symbols from re-linked stdlib objects
                cmd.arg("/FORCE:MULTIPLE");
            }
            match format {
                OutputFormat::Text => println!("Linking geisterhand (in-process fuzzer)"),
                OutputFormat::Json => {}
            }
        } else {
            return Err(anyhow!(
                "Failed to build geisterhand libraries. Check that Perry source crates are available."
            ));
        }
    }

    // Build and link external native libraries from perry.nativeLibrary manifests.
    // Swift sources are deduplicated across the loop — modules sharing the same
    // package.json all see the same swift_sources entries, but each file should
    // be compiled + linked once. Without this, swift's mangled symbols for
    // structs/classes duplicate N times.
    let mut seen_swift_sources: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for native_lib in &ctx.native_libraries {
        if let Some(ref target_config) = native_lib.target_config {
            match format {
                OutputFormat::Text => println!("Building native library: {} ...", native_lib.module),
                OutputFormat::Json => {}
            }

            // Build the Rust crate
            let cargo_toml = target_config.crate_path.join("Cargo.toml");
            if cargo_toml.exists() {
                // Tier 3 targets (tvOS, watchOS) need nightly + build-std
                let is_tier3 = matches!(target,
                    Some("tvos") | Some("tvos-simulator") |
                    Some("watchos") | Some("watchos-simulator"));

                let mut cargo_cmd = Command::new("cargo");
                if is_tier3 {
                    cargo_cmd.arg("+nightly");
                }
                cargo_cmd.arg("build").arg("--release")
                    .arg("--manifest-path").arg(&cargo_toml);

                if let Some(triple) = rust_target_triple(target) {
                    cargo_cmd.arg("--target").arg(triple);
                }

                if is_tier3 {
                    // Match perry-runtime's std build flags exactly so the std
                    // rlibs are bit-identical and dedupe at link time. Without
                    // this, native libs pull in a parallel std with different
                    // metadata hashes and the final Swift-driven link fails
                    // with hundreds of duplicate-symbol errors.
                    cargo_cmd.arg("-Zbuild-std=std,panic_abort");
                }

                // For Android, ensure 16 KB page size alignment (required by Google Play)
                if is_android {
                    cargo_cmd.env("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS",
                        "-C link-arg=-Wl,-z,max-page-size=16384");
                }

                let cargo_status = cargo_cmd.status()?;
                if !cargo_status.success() {
                    return Err(anyhow!(
                        "Failed to build native library crate for {}: {}",
                        native_lib.module,
                        target_config.crate_path.display()
                    ));
                }
            }

            // Find and link the static library
            let lib_name = &target_config.lib_name;
            if !lib_name.is_empty() {
                // Search in the crate's target directory first, then standard paths
                let mut lib_path = None;
                let crate_target_dir = target_config.crate_path.join("target");
                if let Some(triple) = rust_target_triple(target) {
                    let candidate = crate_target_dir.join(triple).join("release").join(lib_name);
                    if candidate.exists() {
                        lib_path = Some(candidate);
                    }
                } else {
                    let candidate = crate_target_dir.join("release").join(lib_name);
                    if candidate.exists() {
                        lib_path = Some(candidate);
                    }
                }

                if let Some(lib) = lib_path {
                    // For shared libraries (.so) on Android, use -L/-l so the linker
                    // records just the soname (not the full build path) in DT_NEEDED.
                    if is_android && lib_name.ends_with(".so") {
                        if let Some(dir) = lib.parent() {
                            cmd.arg(format!("-L{}", dir.display()));
                        }
                        // Strip "lib" prefix and ".so" suffix for -l flag
                        let stem = lib_name.strip_prefix("lib").unwrap_or(lib_name);
                        let stem = stem.strip_suffix(".so").unwrap_or(stem);
                        cmd.arg(format!("-l{}", stem));
                    } else {
                        // When building a plugin host on macOS, force-load plugin-related native
                        // libraries so their symbols are available for dlopen'd plugin dylibs.
                        let force_load = cfg!(target_os = "macos")
                            && ctx.needs_plugins
                            && native_lib.module.contains("plugin");
                        if force_load {
                            cmd.arg(format!("-Wl,-force_load,{}", lib.display()));
                        } else if is_windows && lib.extension().map_or(false, |e| e == "lib") {
                            // On Windows, link native staticlibs directly —
                            // /FORCE:MULTIPLE handles duplicate symbols.
                            cmd.arg(&lib);
                        } else {
                            cmd.arg(&lib);
                        }
                    }
                    match format {
                        OutputFormat::Text => println!("Linking native library: {}", lib.display()),
                        OutputFormat::Json => {}
                    }
                } else {
                    return Err(anyhow!(
                        "Native library {} not found after building {} crate",
                        lib_name, native_lib.module
                    ));
                }
            }

            // Add platform frameworks
            for framework in &target_config.frameworks {
                cmd.arg("-framework").arg(framework);
            }

            // Add platform libraries
            for lib in &target_config.libs {
                if is_windows {
                    cmd.arg(format!("{}.lib", lib));
                } else {
                    cmd.arg(format!("-l{}", lib));
                }
            }

            // Add pkg-config libraries
            for pkg in &target_config.pkg_config {
                if let Ok(output) = Command::new("pkg-config").args(["--libs", pkg]).output() {
                    if output.status.success() {
                        let libs = String::from_utf8_lossy(&output.stdout);
                        for flag in libs.trim().split_whitespace() {
                            cmd.arg(flag);
                        }
                    }
                }
            }

            // Compile manifest-declared Swift sources to object files and
            // append them to the link line. Used by `--features watchos-swift-app`
            // so a native lib can ship its own `@main struct App: App`.
            if !target_config.swift_sources.is_empty() {
                if !is_watchos {
                    return Err(anyhow!(
                        "perry.nativeLibrary.targets.<target>.swift_sources is only supported on watchos/watchos-simulator"
                    ));
                }
                let swift_sdk = if target == Some("watchos-simulator") {
                    "watchsimulator"
                } else {
                    "watchos"
                };
                let swift_triple = if target == Some("watchos-simulator") {
                    "arm64-apple-watchos10.0-simulator"
                } else {
                    "arm64_32-apple-watchos10.0"
                };
                let swift_sysroot = String::from_utf8(
                    Command::new("xcrun")
                        .args(["--sdk", swift_sdk, "--show-sdk-path"])
                        .output()?
                        .stdout,
                )?
                .trim()
                .to_string();
                let swiftc = String::from_utf8(
                    Command::new("xcrun")
                        .args(["--sdk", swift_sdk, "--find", "swiftc"])
                        .output()?
                        .stdout,
                )?
                .trim()
                .to_string();

                let swift_obj_dir = std::env::temp_dir()
                    .join(format!("perry_swift_{}", std::process::id()));
                std::fs::create_dir_all(&swift_obj_dir).ok();

                for swift_src in &target_config.swift_sources {
                    if !swift_src.exists() {
                        return Err(anyhow!(
                            "Swift source not found: {} (declared in {}'s nativeLibrary.swift_sources)",
                            swift_src.display(),
                            native_lib.module
                        ));
                    }
                    let canonical = swift_src.canonicalize().unwrap_or_else(|_| swift_src.clone());
                    if !seen_swift_sources.insert(canonical) {
                        continue;
                    }
                    let stem = swift_src
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("swift_src");
                    let obj_out = swift_obj_dir.join(format!("{}.o", stem));
                    let status = Command::new(&swiftc)
                        .arg("-target").arg(swift_triple)
                        .arg("-sdk").arg(&swift_sysroot)
                        .arg("-parse-as-library")
                        .arg("-emit-object")
                        .arg("-O")
                        .arg("-o").arg(&obj_out)
                        .arg(swift_src)
                        .status()?;
                    if !status.success() {
                        return Err(anyhow!(
                            "Failed to compile Swift source: {}",
                            swift_src.display()
                        ));
                    }
                    cmd.arg(&obj_out);
                    match format {
                        OutputFormat::Text => println!("Linking Swift object: {}", obj_out.display()),
                        OutputFormat::Json => {}
                    }
                }
            }

            // Metal sources are compiled + packed into <app>.app/default.metallib
            // after the `.app` bundle is created below. Just validate the target
            // here so we fail early with a clear message instead of silently
            // dropping shaders on non-Apple-bundle targets.
            if !target_config.metal_sources.is_empty()
                && !matches!(target,
                    Some("ios") | Some("ios-simulator") |
                    Some("tvos") | Some("tvos-simulator") |
                    Some("watchos") | Some("watchos-simulator"))
            {
                return Err(anyhow!(
                    "perry.nativeLibrary.targets.<target>.metal_sources is only supported on ios / ios-simulator / tvos / tvos-simulator / watchos / watchos-simulator"
                ));
            }
        }
    }

    let status = cmd.status()?;

    if !status.success() {
        return Err(anyhow!("Linking failed"));
    }

    Ok(())
}
