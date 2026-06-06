//! Per-platform linker-command construction.
//!
//! Split out of `link/mod.rs` so the orchestrator function stays focused on
//! the link-line assembly (objects, libs, frameworks, native libs). This
//! module owns the `if/else if` chain that picks the right toolchain
//! (clang / swiftc / lld / link.exe / ld64.lld) and primes it with the
//! cross-compile flags, sysroots, and entry-symbol rewrites that each
//! platform needs before any of the per-link-line code runs.

use super::*;

/// Construct the platform-specific linker `Command` and prime it with the
/// toolchain/sysroot/triple flags that every per-platform branch needs
/// before the orchestrator appends object files and libraries.
///
/// Returns an error for unsupported cross-compile combinations (e.g.
/// visionOS from a non-macOS host).
pub fn select_linker_command(
    args_input: &Path,
    ctx: &CompilationContext,
    target: Option<&str>,
    obj_paths: &[PathBuf],
    compiled_features: &[String],
    is_ios: bool,
    is_visionos: bool,
    is_android: bool,
    is_harmonyos: bool,
    is_linux: bool,
    is_windows: bool,
    is_cross_windows: bool,
    is_cross_ios: bool,
    is_cross_visionos: bool,
    is_cross_macos: bool,
    is_watchos: bool,
    is_tvos: bool,
    is_cross_tvos: bool,
) -> Result<Command> {
    let _ = ctx; // reserved for future per-platform context-driven flags
                 // For cross-compilation targets, use the appropriate toolchain
    let cmd = if is_watchos {
        let is_watchos_game_loop = compiled_features.iter().any(|f| f == "watchos-game-loop");
        let is_watchos_swift_app = compiled_features.iter().any(|f| f == "watchos-swift-app");
        let sdk = if target == Some("watchos-simulator") {
            "watchsimulator"
        } else {
            "watchos"
        };
        let sysroot = String::from_utf8(
            Command::new("xcrun")
                .args(["--sdk", sdk, "--show-sdk-path"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
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
        let input_stem = args_input
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| format!("{}_ts", s))
            .unwrap_or_else(|| "main_ts".to_string());
        if let Some(entry_obj) = obj_paths.iter().find(|f| {
            f.file_stem()
                .and_then(|s| s.to_str())
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
                Command::new("xcrun")
                    .args(["--sdk", sdk, "--find", "clang"])
                    .output()?
                    .stdout,
            )?
            .trim()
            .to_string();
            let mut c = Command::new(clang);
            c.arg("-target").arg(triple).arg("-isysroot").arg(&sysroot);
            c
        } else if is_watchos_swift_app {
            // Swift-app: the native lib ships its own `@main struct App: App`
            // (compiled separately in the native-lib loop below). Perry does
            // not emit PerryWatchApp.swift and does not provide a C main.
            // Use swiftc as the linker so Swift stdlib auto-links.
            let swiftc = String::from_utf8(
                Command::new("xcrun")
                    .args(["--sdk", sdk, "--find", "swiftc"])
                    .output()?
                    .stdout,
            )?
            .trim()
            .to_string();
            let mut c = Command::new(swiftc);
            c.arg("-target")
                .arg(triple)
                .arg("-sdk")
                .arg(&sysroot)
                .arg("-parse-as-library")
                // perry-runtime and the native lib each pull in their own std
                // rlibs (Cargo's metadata hashing differs across workspaces even
                // when -Zbuild-std flags match). Tell ld to take first-wins on
                // duplicates rather than fail the link.
                .arg("-Xlinker")
                .arg("-ld_classic");
            c
        } else {
            let swiftc = String::from_utf8(
                Command::new("xcrun")
                    .args(["--sdk", sdk, "--find", "swiftc"])
                    .output()?
                    .stdout,
            )?
            .trim()
            .to_string();
            let swift_runtime = find_watchos_swift_runtime()
                .ok_or_else(|| anyhow!(
                    "PerryWatchApp.swift not found. Expected next to perry binary or in source tree."
                ))?;
            let mut c = Command::new(swiftc);
            c.arg("-target")
                .arg(triple)
                .arg("-sdk")
                .arg(&sysroot)
                .arg("-parse-as-library")
                .arg(&swift_runtime);
            c
        }
    } else if is_visionos && is_cross_visionos {
        return Err(anyhow!(
            "Local visionOS compilation requires Xcode on macOS. Use a macOS host or Perry Hub remote build."
        ));
    } else if is_visionos {
        let sdk = if target == Some("visionos-simulator") {
            "xrsimulator"
        } else {
            "xros"
        };
        let swiftc = String::from_utf8(
            Command::new("xcrun")
                .args(["--sdk", sdk, "--find", "swiftc"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun")
                .args(["--sdk", sdk, "--show-sdk-path"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
        let sdk_version = apple_sdk_version(sdk).unwrap_or_else(|| "1.0".to_string());
        let triple = if target == Some("visionos-simulator") {
            format!("arm64-apple-xros{}-simulator", sdk_version)
        } else {
            format!("arm64-apple-xros{}", sdk_version)
        };
        let swift_runtime = find_visionos_swift_runtime().ok_or_else(|| {
            anyhow!(
                "PerryVisionApp.swift not found. Expected next to perry binary or in source tree."
            )
        })?;

        let input_stem = args_input
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| format!("{}_ts", s))
            .unwrap_or_else(|| "main_ts".to_string());
        if let Some(entry_obj) = obj_paths.iter().find(|f| {
            f.file_stem()
                .and_then(|s| s.to_str())
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
        c.arg("-target")
            .arg(&triple)
            .arg("-sdk")
            .arg(&sysroot)
            .arg("-parse-as-library")
            .arg(&swift_runtime);
        c
    } else if is_ios && is_cross_ios {
        // Cross-compile iOS from Linux using ld64.lld + Apple SDK sysroot
        let ld64 = find_llvm_tool("ld64.lld")
            .or_else(|| {
                // Check common paths
                for p in &[
                    "/usr/local/bin/ld64.lld",
                    "/usr/bin/ld64.lld-18",
                    "/usr/bin/ld64.lld",
                ] {
                    if std::path::Path::new(p).exists() {
                        return Some(PathBuf::from(p));
                    }
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
        c.arg("-arch")
            .arg("arm64")
            .arg("-platform_version")
            .arg("ios")
            .arg("17.0.0")
            .arg("26.0.0")
            .arg("-syslibroot")
            .arg(&sysroot)
            .arg("-L")
            .arg(format!("{}/usr/lib", sysroot))
            .arg("-L")
            .arg(format!("{}/usr/lib/swift", sysroot))
            .arg("-F")
            .arg(format!("{}/System/Library/Frameworks", sysroot))
            .arg("-lSystem")
            .arg("-dead_strip");
        c
    } else if is_ios {
        let sdk = if target == Some("ios-simulator") {
            "iphonesimulator"
        } else {
            "iphoneos"
        };
        let clang = String::from_utf8(
            Command::new("xcrun")
                .args(["--sdk", sdk, "--find", "clang"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun")
                .args(["--sdk", sdk, "--show-sdk-path"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
        let triple = if target == Some("ios-simulator") {
            "arm64-apple-ios17.0-simulator"
        } else {
            "arm64-apple-ios17.0"
        };

        // Discover Xcode developer directory for Swift standard library paths.
        // Swift libs live in the toolchain, not the SDK sysroot, so the linker
        // needs explicit -L flags to resolve auto-linked libs like swiftCore.
        let developer_dir =
            String::from_utf8(Command::new("xcode-select").arg("-p").output()?.stdout)?
                .trim()
                .to_string();

        let mut c = Command::new(clang);
        c.arg("-target")
            .arg(triple)
            .arg("-isysroot")
            .arg(&sysroot)
            // Swift standard library .tbd stubs in the SDK (swiftCore, swift_Concurrency, etc.)
            .arg("-L")
            .arg(format!("{}/usr/lib/swift", sysroot))
            // Swift compatibility static archives in the toolchain
            .arg("-L")
            .arg(format!(
                "{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/{}",
                developer_dir, sdk
            ))
            // Native C++ deps (e.g. Jolt physics) pull in libc++ / libc++abi
            // symbols; clang links these only when it sees C++ inputs, which it
            // doesn't here (we hand it objects + .a archives), so request them
            // explicitly. Mirrors the cross-iOS branch.
            .arg("-lc++")
            .arg("-lc++abi");
        c
    } else if is_tvos && is_cross_tvos {
        // Cross-compile tvOS from Linux using ld64.lld + Apple SDK sysroot.
        // The Linux builder worker ships a sysroot at /opt/apple-sysroot/tvos
        // (symlinked to the iOS sysroot — tvOS headers/libs are compatible with
        // the iOS SDK on aarch64 for our usage).
        let ld64 = find_llvm_tool("ld64.lld")
            .or_else(|| {
                // Check common paths
                for p in &[
                    "/usr/local/bin/ld64.lld",
                    "/usr/bin/ld64.lld-18",
                    "/usr/bin/ld64.lld",
                ] {
                    if std::path::Path::new(p).exists() {
                        return Some(PathBuf::from(p));
                    }
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
        c.arg("-arch")
            .arg("arm64")
            .arg("-platform_version")
            .arg("tvos")
            .arg("17.0.0")
            .arg("26.0.0")
            .arg("-syslibroot")
            .arg(&sysroot)
            .arg("-L")
            .arg(format!("{}/usr/lib", sysroot))
            .arg("-L")
            .arg(format!("{}/usr/lib/swift", sysroot))
            .arg("-F")
            .arg(format!("{}/System/Library/Frameworks", sysroot))
            .arg("-lSystem")
            .arg("-dead_strip");
        c
    } else if is_tvos {
        let sdk = if target == Some("tvos-simulator") {
            "appletvsimulator"
        } else {
            "appletvos"
        };
        let clang = String::from_utf8(
            Command::new("xcrun")
                .args(["--sdk", sdk, "--find", "clang"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
        let sysroot = String::from_utf8(
            Command::new("xcrun")
                .args(["--sdk", sdk, "--show-sdk-path"])
                .output()?
                .stdout,
        )?
        .trim()
        .to_string();
        let triple = if target == Some("tvos-simulator") {
            "arm64-apple-tvos17.0-simulator"
        } else {
            "arm64-apple-tvos17.0"
        };

        let developer_dir =
            String::from_utf8(Command::new("xcode-select").arg("-p").output()?.stdout)?
                .trim()
                .to_string();

        let mut c = Command::new(clang);
        c.arg("-target")
            .arg(triple)
            .arg("-isysroot")
            .arg(&sysroot)
            .arg("-L")
            .arg(format!("{}/usr/lib/swift", sysroot))
            .arg("-L")
            .arg(format!(
                "{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/{}",
                developer_dir, sdk
            ));
        c
    } else if is_android {
        // Use Android NDK clang to produce a shared library (.so)
        let ndk_home = std::env::var("ANDROID_NDK_HOME").map_err(|_| {
            anyhow!(
                "ANDROID_NDK_HOME not set. Set it to your NDK path, e.g. \
                 $HOME/Library/Android/sdk/ndk/28.0.12433566 (macOS), \
                 $HOME/Android/Sdk/ndk/28.0.12433566 (Linux), or \
                 %LOCALAPPDATA%\\Android\\Sdk\\ndk\\28.0.12433566 (Windows)"
            )
        })?;
        // #1508: Windows host falls through to "linux-x86_64" and points at
        // a path that doesn't exist on the NDK. The NDK ships per-host
        // prebuilt toolchains under `toolchains/llvm/prebuilt/<host>/`;
        // the host tag must match the build machine, not the target.
        let host_tag = if cfg!(target_os = "macos") {
            "darwin-x86_64"
        } else if cfg!(target_os = "windows") {
            "windows-x86_64"
        } else {
            "linux-x86_64"
        };
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
            .arg("-target")
            .arg("aarch64-linux-android24")
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
        let sdk = super::super::library_search::find_harmonyos_sdk().ok_or_else(|| {
            anyhow!(
                "OHOS SDK not found. Install DevEco Studio or the standalone \
             OpenHarmony SDK from https://developer.huawei.com/consumer/en/develop \
             and set OHOS_SDK_HOME to the SDK root (the dir that contains \
             native/llvm/bin/clang and native/sysroot/)."
            )
        })?;
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
        // When cross-compiling from macOS, use clang + ld.lld + a glibc
        // sysroot pointed to by PERRY_LINUX_SYSROOT (matching the
        // PERRY_IOS_SYSROOT/PERRY_WINDOWS_SYSROOT builder pattern).
        let mut c = Command::new("cc");
        #[cfg(not(target_os = "linux"))]
        {
            c.arg("-target").arg("x86_64-unknown-linux-gnu");
            c.arg("-fuse-ld=lld");
            if let Ok(sysroot) = std::env::var("PERRY_LINUX_SYSROOT") {
                c.arg(format!("--sysroot={}", sysroot));
                c.arg(format!("-L{}/usr/lib/x86_64-linux-gnu", sysroot));
                c.arg(format!("-L{}/lib/x86_64-linux-gnu", sysroot));
                let gcc_root = format!("{}/usr/lib/gcc/x86_64-linux-gnu", sysroot);
                if let Ok(entries) = std::fs::read_dir(&gcc_root) {
                    if let Some(version) = entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.file_name().into_string().unwrap_or_default())
                        .filter(|n| n.chars().all(|c| c.is_ascii_digit()))
                        .max()
                    {
                        let gcc_dir = format!("{}/{}", gcc_root, version);
                        c.arg(format!("-L{}", gcc_dir));
                        c.arg(format!("-B{}", gcc_dir));
                    }
                }
                c.arg("-Wl,-dynamic-linker=/lib64/ld-linux-x86-64.so.2");
            }
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
        // The `--windows-subsystem` / `[windows] subsystem` override (resolved
        // into ctx.windows_subsystem) can force GUI/console past the heuristic.
        c.arg(windows_pe_subsystem_flag(
            windows_subsystem_needs_ui(&ctx.windows_subsystem, ctx.needs_ui),
            &ctx.min_windows_version,
        ))
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
                for p in &[
                    "/usr/local/bin/ld64.lld",
                    "/usr/bin/ld64.lld-18",
                    "/usr/bin/ld64.lld",
                ] {
                    if std::path::Path::new(p).exists() {
                        return Some(PathBuf::from(p));
                    }
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
        c.arg("-arch")
            .arg("arm64")
            .arg("-platform_version")
            .arg("macos")
            .arg("13.0.0")
            .arg("26.0.0")
            .arg("-syslibroot")
            .arg(&sysroot)
            .arg("-L")
            .arg(format!("{}/usr/lib", sysroot))
            .arg("-L")
            .arg(format!("{}/usr/lib/swift", sysroot))
            .arg("-F")
            .arg(format!("{}/System/Library/Frameworks", sysroot))
            .arg("-lSystem")
            .arg("-dead_strip");
        c
    } else {
        Command::new("cc")
    };

    Ok(cmd)
}
