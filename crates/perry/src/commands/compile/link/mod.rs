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
//!
//! Wave 4 split (v0.5.x) — the per-platform linker-`Command` selection (~615
//! LOC of `if is_watchos { swiftc … } else if is_ios { clang … } else if …`)
//! moved into `platform_cmd.rs` so this file stays under the 2k-LOC soft
//! ceiling. The orchestrator below still drives the full link line; only the
//! initial `Command::new(<toolchain>)` + sysroot/triple/entry-rewrite prelude
//! lives in the sibling module.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OutputFormat;

use super::{
    apple_sdk_version, build_geisterhand_libs, find_geisterhand_library, find_geisterhand_runtime,
    find_geisterhand_ui, find_lld_link, find_llvm_tool, find_msvc_lib_paths, find_msvc_link_exe,
    find_perry_windows_sdk, find_stdlib_library, find_ui_library, find_visionos_swift_runtime,
    find_watchos_swift_runtime, rust_target_triple, strip_duplicate_objects_from_lib,
    windows_pe_subsystem_flag, CompilationContext,
};

mod platform_cmd;

pub use platform_cmd::select_linker_command;

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
    // #466 Phase 4 step 2: well-known native binding archives. Added
    // to the link line right after `stdlib_lib`. The matching
    // perry-stdlib feature was already stripped during the auto-
    // optimize rebuild, so the resulting link contains exactly one
    // copy of each `_js_<package>_*` symbol — no duplicates.
    well_known_libs: &[PathBuf],
    jsruntime_lib: &Option<PathBuf>,
    // Issue #76 — `libperry_wasm_host.a` (wasmi-backed WebAssembly host
    // runtime). Only `Some(...)` when the user passed `--enable-wasm-runtime`
    // and the archive was located. Appended to the link command after the
    // jsruntime/stdlib block so the linker resolves `perry_wasm_host_*`
    // symbols referenced by `js_webassembly_*` shims in `perry-runtime`.
    wasm_host_lib: &Option<PathBuf>,
    exe_path: &Path,
    format: OutputFormat,
    // `--debug-symbols`: keep symbols / emit a PDB so RUST_BACKTRACE
    // panics in the compiled app symbolize. Windows-active today.
    debug_symbols: bool,
) -> Result<()> {
    // #498 - supply-chain gate. Before any prebuilt archive hits the
    // linker, hash it and compare against `perry.lock`. First build
    // writes the lockfile; subsequent builds verify. Mismatch fails
    // with an actionable diagnostic (`perry lock --update <pkg>`).
    // `PERRY_LOCK_FROZEN=1` upgrades the verify to CI mode (refuses
    // to extend the lock); `PERRY_LOCK_UPDATE=<pkg>` deliberately
    // bumps the named package's hashes. Wired here so every backend
    // (LLVM / WASM / ArkTS / HarmonyOS / Glance / SwiftUI / JS)
    // inherits the gate from one chokepoint.
    super::run_lock_verify_for_compile(ctx, target)?;

    let is_ios = matches!(target, Some("ios-simulator") | Some("ios"));
    let is_visionos = matches!(target, Some("visionos-simulator") | Some("visionos"));
    let is_android = matches!(target, Some("android"));
    let is_harmonyos = matches!(target, Some("harmonyos") | Some("harmonyos-simulator"));
    let is_linux =
        matches!(target, Some("linux")) || (target.is_none() && cfg!(target_os = "linux"));
    let is_windows =
        matches!(target, Some("windows")) || (target.is_none() && cfg!(target_os = "windows"));
    let is_cross_windows = is_windows && !cfg!(target_os = "windows");
    let is_cross_ios = is_ios && !cfg!(target_os = "macos");
    let is_cross_visionos = is_visionos && !cfg!(target_os = "macos");
    let is_cross_macos = matches!(target, Some("macos")) && !cfg!(target_os = "macos");
    let is_watchos = matches!(target, Some("watchos") | Some("watchos-simulator"));
    let is_tvos = matches!(target, Some("tvos") | Some("tvos-simulator"));
    let is_cross_tvos = is_tvos && !cfg!(target_os = "macos");

    let mut cmd = select_linker_command(
        args_input,
        ctx,
        target,
        obj_paths,
        compiled_features,
        is_ios,
        is_visionos,
        is_android,
        is_harmonyos,
        is_linux,
        is_windows,
        is_cross_windows,
        is_cross_ios,
        is_cross_visionos,
        is_cross_macos,
        is_watchos,
        is_tvos,
        is_cross_tvos,
    )?;

    // When ios-game-loop is enabled, rename _main to _perry_user_main in the
    // entry object file so the perry runtime's main() (from ios_game_loop.rs)
    // becomes the process entry point. It spawns _perry_user_main on a game thread.
    if (is_ios || is_tvos) && compiled_features.iter().any(|f| f == "ios-game-loop") {
        if let Some(entry_obj) = obj_paths
            .iter()
            .find(|f| f.to_string_lossy().contains("main_ts"))
        {
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
                if let Ok(walker) = walkdir::WalkDir::new(&out_dir)
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()
                {
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
            println!(
                "  harmonyos: linking {} build.rs native object(s)",
                native_objs.len()
            );
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
        cmd.arg("/OPT:REF");
        if debug_symbols {
            // `/DEBUG` makes lld-link emit a PDB next to the .exe from the
            // debug info already present in the input objects/libs. Without
            // it, perry binaries have no symbol table and a RUST_BACKTRACE
            // panic is an unreadable list of `<unknown>` — there is no other
            // way to diagnose a runtime crash in a compiled Windows app.
            // Skip /OPT:ICF here: COMDAT folding collapses distinct
            // identical-bodied functions to one symbol, which would make the
            // very backtrace this flag exists to produce ambiguous.
            cmd.arg("/DEBUG");
        } else {
            cmd.arg("/OPT:ICF");
        }
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
        && (ctx.needs_ui || is_watchos)
        && find_ui_library(target).is_some();
    if !skip_runtime {
        if let Some(ref jsruntime) = jsruntime_lib {
            // Issue #257: when user code only references V8 FFIs that have stubs in
            // perry-runtime (`js_load_module` + `js_call_function` + …, but NOT
            // `js_call_method`), Mach-O lazy archive resolution satisfies those
            // symbols against the (now-weak) closure.rs stubs and never pulls
            // `interop.o` from libperry_jsruntime.a — yielding a binary that
            // silently links V8 nowhere even though the driver thought it linked
            // jsruntime. Force-keep `js_runtime_shutdown` (a no_mangle symbol that
            // exists ONLY in jsruntime/interop.rs, never stubbed in runtime) via
            // `-Wl,-u`, which tells the linker to treat it as undefined. The
            // resolver then has to pull `interop.o` from jsruntime.a, which
            // cascades V8/deno_core deps and gives strong defs of all V8 FFIs that
            // override the weak runtime stubs. ELF gets `--undefined=` (same
            // semantics, different syntax). Windows links jsruntime via standard
            // archive scan and doesn't have the same weak-shadow issue.
            if !is_windows {
                let undef_flag = if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
                    "-Wl,-u,_js_runtime_shutdown"
                } else {
                    "-Wl,--undefined=js_runtime_shutdown"
                };
                cmd.arg(undef_flag);
            }
            cmd.arg(jsruntime);
            // Also link runtime to supply symbols DCE'd from jsruntime (e.g. js_register_class_method)
            if !is_android && !is_windows {
                cmd.arg(runtime_lib);
            }
            // Issue #886 — express (or any compilePackages program that
            // mixes V8-fallback JS modules with natively-compiled TS that
            // emits a `perry-ext-*` FFI) reaches this branch. Pre-fix:
            // jsruntime bundles perry-stdlib at default features but NO
            // `perry-ext-*` staticlib, so a `js_node_http_create_server`
            // (or `js_node_http_res_end`, …) reference from express's
            // compiled `application.js` link-failed even though
            // `auto-optimize: built …/libperry_ext_http.a` had just run.
            // The codegen-side FFI provenance registry
            // (`crates/perry-codegen/src/ext_registry.rs`) drains
            // `OwnerKind::WellKnown("http")` into
            // `ctx.native_module_imports`, the auto-optimize layer builds
            // perry-ext-http.a + puts it into `well_known_libs`, but the
            // OLD jsruntime arm here completely skipped both `stdlib_lib`
            // and `well_known_libs`. Mirror the parallel `else if
            // ctx.needs_stdlib` branch by appending both after jsruntime
            // + runtime: archive scan is first-definition-wins on Mach-O
            // and ELF, so jsruntime's bundled-stdlib copies still win for
            // duplicate `js_*` symbols (yielding the expected
            // "ld: warning: duplicate symbol" lines this is documented
            // to produce), and the perry-ext-* libs cover the gaps that
            // neither jsruntime nor its bundled stdlib carries.
            //
            // The auto-optimize stdlib is included alongside well-known
            // libs because the well-known flip strips features from it
            // (e.g. `http-client` when routing to perry-ext-http) — the
            // auto-built stdlib still carries the OTHER stdlib FFIs the
            // program needs, so including both is safe and necessary.
            if ctx.needs_stdlib {
                if let Some(ref stdlib) = stdlib_lib {
                    cmd.arg(stdlib);
                }
                for wk in well_known_libs {
                    cmd.arg(wk);
                }
            }
        } else if ctx.needs_stdlib || is_windows {
            // On Windows/MSVC, always try to link stdlib because codegen unconditionally
            // declares all stdlib extern functions, creating import references that MSVC
            // won't dead-strip. On macOS/Linux, the linker ignores unreferenced archives.
            if let Some(ref stdlib) = stdlib_lib {
                // Windows: link the standalone perry_runtime.lib FIRST so
                // its symbols win lld-link's /FORCE:MULTIPLE "first
                // definition wins" rule over the perry-runtime copies
                // *bundled* inside perry_stdlib.lib and the
                // /WHOLEARCHIVE'd perry_ui_windows.lib. Auto-optimize
                // refreshes perry-runtime + perry-stdlib but NOT
                // perry-ui-windows, so the UI lib's bundled runtime is
                // perpetually stale; the /WHOLEARCHIVE force-includes its
                // js_* symbols, and without this the stale copy shadows a
                // genuine runtime fix (e.g. the js_shadow_frame_pop bounds
                // guard, #880) — the crash it fixes still fires because the
                // guarded function never gets linked. The standalone
                // runtime_lib is the canonical / auto-optimize-fresh
                // source; making it authoritative on Windows matches every
                // other platform (all of which already link runtime_lib).
                if is_windows {
                    cmd.arg(runtime_lib);
                }
                cmd.arg(stdlib);
                // #466 Phase 4 step 2: well-known bindings join the
                // link line right after perry-stdlib so they cover
                // the exact `_js_*` symbol gap that was just opened
                // by stripping the corresponding feature from the
                // perry-stdlib rebuild.
                for wk in well_known_libs {
                    cmd.arg(wk);
                }
                // Also link runtime to supply symbols that may be DCE'd from stdlib's
                // bundled perry-runtime (e.g. js_closure_unbind_this, js_string_addref)
                if !is_android && !is_windows {
                    cmd.arg(runtime_lib);
                }
            } else {
                if ctx.needs_stdlib {
                    eprintln!(
                        "Warning: stdlib required but {} not found, using runtime-only",
                        if is_windows {
                            "perry_stdlib.lib"
                        } else {
                            "libperry_stdlib.a"
                        }
                    );
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
            // #466 Phase 4 step 2: see the parallel comment in the
            // non-Android branch above.
            for wk in well_known_libs {
                cmd.arg(wk);
            }
        } else {
            eprintln!("Warning: stdlib required but libperry_stdlib.a not found");
        }
    }

    // Issue #76 — wasmi host runtime, opt-in via `--enable-wasm-runtime`.
    // Append after stdlib/jsruntime so the linker can resolve `perry_wasm_host_*`
    // symbols referenced by the always-present `js_webassembly_*` FFIs in
    // perry-runtime.
    if let Some(ref wasm_host) = wasm_host_lib {
        cmd.arg(wasm_host);
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
        cmd.arg("-o").arg(exe_path).arg("-lc");
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
                "js_array_from_f64",
                "js_array_push_f64",
                "js_bigint_is_zero",
                "js_closure_alloc",
                "js_console_log_spread",
                "js_dynamic_object_get_property",
                "js_dynamic_string_equals",
                "js_gc_register_global_root",
                "js_is_truthy",
                "js_jsvalue_compare",
                "js_jsvalue_equals",
                "js_nanbox_get_pointer",
                "js_nanbox_pointer",
                "js_nanbox_string",
                "js_native_call_method",
                "js_object_alloc_class_with_keys",
                "js_object_alloc_with_shape",
                "js_register_class_method",
                "js_string_char_code_at",
                "js_string_from_bytes",
                "js_string_length",
                "perry_debug_trace_init",
                "perry_debug_trace_init_done",
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
        cmd.arg("-framework")
            .arg("WatchKit")
            .arg("-framework")
            .arg("Foundation")
            .arg("-framework")
            .arg("CoreFoundation")
            .arg("-framework")
            .arg("Security")
            .arg("-lSystem")
            .arg("-lresolv");
        if is_watchos_game_loop {
            // QuartzCore for CAMetalLayer-backed rendering (Metal.framework is NOT
            // in the watchOS SDK — the native lib must dlopen it or supply its own
            // path to the device's Metal dylib). -lobjc for the dynamic
            // WKApplicationDelegate class registered from watchos_game_loop.rs.
            cmd.arg("-framework").arg("QuartzCore").arg("-lobjc");
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
        cmd.arg("-framework")
            .arg("UIKit")
            .arg("-framework")
            .arg("Foundation")
            .arg("-framework")
            .arg("CoreGraphics")
            .arg("-framework")
            .arg("Security")
            .arg("-framework")
            .arg("CoreFoundation")
            .arg("-framework")
            .arg("SystemConfiguration")
            .arg("-framework")
            .arg("QuartzCore")
            .arg("-framework")
            .arg("AVFAudio") // AVAudioEngine for audio capture
            .arg("-framework")
            .arg("AVFoundation") // Camera capture (AVCaptureSession)
            .arg("-framework")
            .arg("CoreMedia") // CMSampleBuffer
            .arg("-framework")
            .arg("CoreVideo") // CVPixelBuffer
            .arg("-framework")
            .arg("UserNotifications") // UNUserNotificationCenter (perry/system notificationSend)
            .arg("-framework")
            .arg("CoreLocation") // CLCircularRegion for UNLocationNotificationTrigger (#96)
            .arg("-framework")
            .arg("MediaPlayer") // perry/media — Now Playing + Remote Command Center
            .arg("-framework")
            .arg("MapKit") // perry/ui MapView (#517) — MKMapView
            .arg("-framework")
            .arg("PDFKit") // perry/ui PdfView (#516) — PDFView
            .arg("-framework")
            .arg("BackgroundTasks") // perry/background BGTaskScheduler (#538)
            .arg("-framework")
            .arg("Network") // perry/system network reachability (#582)
            .arg("-liconv")
            .arg("-lresolv")
            .arg("-lobjc")
            .arg("-lSystem");
    } else if is_visionos {
        cmd.arg("-framework")
            .arg("SwiftUI")
            .arg("-framework")
            .arg("UIKit")
            .arg("-framework")
            .arg("Foundation")
            .arg("-framework")
            .arg("CoreGraphics")
            .arg("-framework")
            .arg("Security")
            .arg("-framework")
            .arg("CoreFoundation")
            .arg("-framework")
            .arg("SystemConfiguration")
            .arg("-framework")
            .arg("QuartzCore")
            .arg("-framework")
            .arg("AVFAudio")
            .arg("-framework")
            .arg("AVFoundation")
            .arg("-framework")
            .arg("CoreMedia")
            .arg("-framework")
            .arg("CoreVideo")
            .arg("-framework")
            .arg("MediaPlayer") // perry/media — Now Playing + Remote Command Center
            .arg("-framework")
            .arg("MapKit") // perry/ui MapView (#517) — MKMapView (visionOS)
            .arg("-framework")
            .arg("PDFKit") // perry/ui PdfView (#516) — PDFView (visionOS)
            .arg("-framework")
            .arg("BackgroundTasks") // perry/background BGTaskScheduler (#538)
            .arg("-liconv")
            .arg("-lresolv")
            .arg("-lobjc")
            .arg("-lSystem");
    } else if is_tvos {
        // tvOS frameworks (UIKit-based, like iOS)
        cmd.arg("-framework")
            .arg("UIKit")
            .arg("-framework")
            .arg("Foundation")
            .arg("-framework")
            .arg("CoreGraphics")
            .arg("-framework")
            .arg("Security")
            .arg("-framework")
            .arg("CoreFoundation")
            .arg("-framework")
            .arg("SystemConfiguration")
            .arg("-framework")
            .arg("QuartzCore")
            .arg("-framework")
            .arg("AVFoundation")
            .arg("-framework")
            .arg("GameController")
            .arg("-framework")
            .arg("Metal")
            .arg("-framework")
            .arg("MapKit") // perry/ui MapView (#517) — MKMapView (tvOS)
            .arg("-framework")
            .arg("MediaPlayer") // perry/media — Now Playing + Siri Remote
            .arg("-framework")
            .arg("BackgroundTasks") // perry/background BGTaskScheduler (#538)
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
        // `libhilog_ndk.z.so` provides OH_LOG_Print, used by Perry's
        // `js_console_log_*` family on harmonyos to route compiled-TS
        // console output to hilog (so Perry-emitted log lines surface
        // in DevEco/hdc the same way ArkTS console.log does), and by the
        // arkts_callbacks bridge for diagnostic register/invoke traces.
        cmd.arg("-lhilog_ndk.z");
        // `libtime_service_ndk.so` provides OH_TimeService_GetTimeZone,
        // referenced by the `iana-time-zone` crate (pulled in transitively
        // by `chrono` etc.) when it detects an OHOS target. The OHOS
        // dynamic loader rejects libentry.so at app launch if this isn't
        // listed in DT_NEEDED. Note: no `.z.` in the soname, unlike the
        // ace_napi / hilog_ndk libs above.
        cmd.arg("-ltime_service_ndk");
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
        std::fs::write(
            &stub_c,
            concat!(
                "typedef int jint;\n",
                "typedef jint jsize;\n",
                "jint JNI_GetCreatedJavaVMs(void **vm_buf, jsize buf_len, jsize *n_vms) {\n",
                "    if (n_vms) *n_vms = 0;\n",
                "    return 0;\n",
                "}\n",
            ),
        )
        .ok();
        let ndk_home = std::env::var("ANDROID_NDK_HOME").unwrap_or_default();
        let host_tag = if cfg!(target_os = "macos") {
            "darwin-x86_64"
        } else {
            "linux-x86_64"
        };
        let ndk_clang = format!(
            "{}/toolchains/llvm/prebuilt/{}/bin/aarch64-linux-android24-clang",
            ndk_home, host_tag
        );
        let stub_ok = Command::new(&ndk_clang)
            .args(["-c", "-fPIC", "-target", "aarch64-linux-android24"])
            .arg("-o")
            .arg(&stub_o)
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
            cmd.arg("-lssl").arg("-lcrypto");
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
            .arg("iphlpapi.lib")
            // winhttp.lib — perry-ui-windows::widgets::image::fetch_url_blocking
            // uses WinHttpOpen/Connect/OpenRequest/SendRequest/ReceiveResponse
            // to fetch Image(url) bytes. The `windows` crate's `Win32_Networking_WinHttp`
            // feature emits #[link] attrs in the rlib, but those don't propagate
            // through perry-ui-windows's `staticlib` crate-type to perry's final
            // link line. Closes #732.
            .arg("winhttp.lib");
    } else {
        // macOS frameworks for runtime (sysinfo, etc.) and V8.
        // Gate on `!is_harmonyos` so the macOS host doesn't leak its
        // frameworks into ELF cross-compile targets that fall through this
        // `else` branch — `cfg!(target_os = "macos")` is true whenever we're
        // running ON macOS, regardless of the actual target.
        if (cfg!(target_os = "macos") || is_cross_macos) && !is_harmonyos {
            cmd.arg("-framework")
                .arg("Security")
                .arg("-framework")
                .arg("CoreFoundation")
                .arg("-framework")
                .arg("SystemConfiguration")
                .arg("-liconv")
                .arg("-lresolv")
                .arg("-lobjc");

            if jsruntime_lib.is_some() {
                cmd.arg("-lc++");
            }
        }

        // On Linux (native, not cross-compiling to macOS), link against system libraries
        if cfg!(target_os = "linux") && !is_cross_macos {
            cmd.arg("-lm").arg("-lpthread").arg("-ldl");

            if ctx.needs_stdlib || jsruntime_lib.is_some() {
                cmd.arg("-lssl").arg("-lcrypto");
            }

            if jsruntime_lib.is_some() {
                cmd.arg("-lstdc++");
            }
        }
    }

    // Issue #607 — watchOS targets always link the UI lib regardless of
    // `ctx.needs_ui`. The watchOS Swift template (`PerryWatchApp.swift`)
    // unconditionally references four `@_silgen_name`'d Rust symbols
    // (`perry_watchos_tree_version` / `perry_watchos_toggle_changed` /
    // `perry_watchos_toast_seq` / `perry_watchos_toast_dismiss`) that
    // live in `libperry_ui_watchos.a`. A console-only TS program has
    // `needs_ui = false`, so the UI lib was previously not added to the
    // link line — leaving those four symbols undefined and the link
    // failing. Forcing the UI lib for watchOS adds ~MBs but unblocks
    // `console.log("ok")`-only programs from compiling.
    let force_ui = is_watchos;
    if ctx.needs_ui || force_ui {
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
                let linux_stdlib_for_ui =
                    stdlib_lib.clone().or_else(|| find_stdlib_library(target));
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
                        "-lgtk-4",
                        "-lgio-2.0",
                        "-lgobject-2.0",
                        "-lglib-2.0",
                        "-lpangocairo-1.0",
                        "-lpango-1.0",
                        "-lharfbuzz",
                        "-lgdk_pixbuf-2.0",
                        "-lcairo-gobject",
                        "-lcairo",
                        "-lgraphene-1.0",
                    ] {
                        cmd.arg(lib);
                    }
                }
                // PulseAudio for audio capture (only needed with UI)
                cmd.arg("-lpulse-simple").arg("-lpulse");
                // GStreamer libs — pulled in by perry-ui-gtk4's gstreamer-rs
                // dep (added in v0.5.440 for the perry/media playbin backend).
                // GTK4's pkg-config doesn't transitively reference the
                // gstreamer-1.0 sonames, so the `-lgstreamer-1.0` (and the
                // base/app/video/audio sublibs that gstreamer-rs's playbin
                // path touches) have to land on the link line explicitly or ld
                // fails with `undefined reference to gst_message_parse_buffering`
                // + `DSO missing from command line` (#423). Same pkg-config →
                // hardcoded-fallback shape as the GTK4 block above.
                let mut got_gst_libs = false;
                let gst_pc_out = Command::new("pkg-config")
                    .args([
                        "--libs",
                        "gstreamer-1.0",
                        "gstreamer-base-1.0",
                        "gstreamer-app-1.0",
                        "gstreamer-video-1.0",
                        "gstreamer-audio-1.0",
                    ])
                    .output();
                if let Ok(ref output) = gst_pc_out {
                    if output.status.success() {
                        let libs = String::from_utf8_lossy(&output.stdout);
                        for flag in libs.trim().split_whitespace() {
                            cmd.arg(flag);
                        }
                        got_gst_libs = true;
                    }
                }
                if !got_gst_libs {
                    eprintln!(
                        "Warning: `pkg-config --libs gstreamer-1.0 ...` did not \
                         return GStreamer linker flags ({}). Falling back to a \
                         hardcoded GStreamer link set — install \
                         `libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev` \
                         (Debian/Ubuntu) or `gstreamer1-devel \
                         gstreamer1-plugins-base-devel` (Fedora/RHEL) to silence \
                         this warning.",
                        match &gst_pc_out {
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
                        "-lgstreamer-1.0",
                        "-lgstbase-1.0",
                        "-lgstapp-1.0",
                        "-lgstvideo-1.0",
                        "-lgstaudio-1.0",
                    ] {
                        cmd.arg(lib);
                    }
                }
                // libshumate — GNOME's GTK4 vector-tile map widget for the
                // perry/ui MapView (#517). Same pkg-config → hardcoded
                // fallback shape as GTK4 / GStreamer above.
                let mut got_shumate_libs = false;
                let shumate_pc_out = Command::new("pkg-config")
                    .args(["--libs", "shumate-1.0"])
                    .output();
                if let Ok(ref output) = shumate_pc_out {
                    if output.status.success() {
                        let libs = String::from_utf8_lossy(&output.stdout);
                        for flag in libs.trim().split_whitespace() {
                            cmd.arg(flag);
                        }
                        got_shumate_libs = true;
                    }
                }
                if !got_shumate_libs {
                    eprintln!(
                        "Warning: `pkg-config --libs shumate-1.0` did not return \
                         libshumate linker flags ({}). Falling back to \
                         `-lshumate-1.0` — install `libshumate-dev` \
                         (Debian/Ubuntu) or `libshumate-devel` (Fedora/RHEL) to \
                         silence this warning.",
                        match &shumate_pc_out {
                            Err(e) => format!("pkg-config not runnable: {e}"),
                            Ok(o) if !o.status.success() => format!(
                                "pkg-config exited {}: {}",
                                o.status.code().unwrap_or(-1),
                                String::from_utf8_lossy(&o.stderr).trim()
                            ),
                            Ok(_) => "no output".to_string(),
                        }
                    );
                    cmd.arg("-lshumate-1.0");
                }
                // WebKitGTK 6.0 + libsoup-3.0 — perry/ui WebView (#658, v0.5.864).
                // perry-ui-gtk4's webkit6/soup3 deps reference symbols like
                // `soup_check_version` from libsoup-3.0 transitively; without
                // explicit `-lsoup-3.0` ld errors with `DSO missing from
                // command line`. Same pkg-config → hardcoded-fallback shape
                // as GTK4 / GStreamer / shumate above.
                let mut got_webkit_libs = false;
                let webkit_pc_out = Command::new("pkg-config")
                    .args(["--libs", "webkitgtk-6.0", "libsoup-3.0"])
                    .output();
                if let Ok(ref output) = webkit_pc_out {
                    if output.status.success() {
                        let libs = String::from_utf8_lossy(&output.stdout);
                        for flag in libs.trim().split_whitespace() {
                            cmd.arg(flag);
                        }
                        got_webkit_libs = true;
                    }
                }
                if !got_webkit_libs {
                    eprintln!(
                        "Warning: `pkg-config --libs webkitgtk-6.0 libsoup-3.0` \
                         did not return WebKitGTK linker flags ({}). Falling \
                         back to a hardcoded set — install `libwebkitgtk-6.0-dev` \
                         (Debian/Ubuntu) which pulls libsoup-3.0-dev + \
                         libjavascriptcoregtk-6.0-dev to silence this warning.",
                        match &webkit_pc_out {
                            Err(e) => format!("pkg-config not runnable: {e}"),
                            Ok(o) if !o.status.success() => format!(
                                "pkg-config exited {}: {}",
                                o.status.code().unwrap_or(-1),
                                String::from_utf8_lossy(&o.stderr).trim()
                            ),
                            Ok(_) => "no output".to_string(),
                        }
                    );
                    for lib in ["-lwebkitgtk-6.0", "-ljavascriptcoregtk-6.0", "-lsoup-3.0"] {
                        cmd.arg(lib);
                    }
                }
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
                                                                  // perry/media — AVPlayer is in AVFoundation (already linked
                                                                  // above). CoreMedia provides CMTime + CMTimeGetSeconds /
                                                                  // CMTimeMakeWithSeconds used for seek + position. MediaPlayer
                                                                  // provides MPNowPlayingInfoCenter / MPRemoteCommandCenter /
                                                                  // MPMediaItemArtwork (lock screen + Touch Bar + Now Playing).
                    cmd.arg("-framework").arg("CoreMedia");
                    cmd.arg("-framework").arg("MediaPlayer");
                    // perry/ui MapView (#517) — MKMapView lives in MapKit.
                    cmd.arg("-framework").arg("MapKit");
                    // perry/ui PdfView (#516) — PDFView lives in PDFKit, which
                    // also exposes the PDFDocument / PDFPage classes used for
                    // page-count + page-navigation queries.
                    cmd.arg("-framework").arg("PDFKit");
                    // perry/system network reachability (#582) — NWPathMonitor.
                    cmd.arg("-framework").arg("Network");
                }
            }

            match format {
                OutputFormat::Text => {
                    println!("Linking perry/ui (native UI) from {}", ui_lib.display())
                }
                OutputFormat::Json => {}
            }
        } else {
            let (lib_name, build_cmd) = if is_watchos {
                (
                    "libperry_ui_watchos.a",
                    "cargo build --release -p perry-ui-watchos --target arm64_32-apple-watchos",
                )
            } else if is_tvos {
                (
                    "libperry_ui_tvos.a",
                    "cargo build --release -p perry-ui-tvos --target aarch64-apple-tvos",
                )
            } else if is_visionos {
                ("libperry_ui_visionos.a", "cargo build --release -p perry-ui-visionos --target aarch64-apple-visionos-sim")
            } else if is_ios {
                (
                    "libperry_ui_ios.a",
                    "cargo build --release -p perry-ui-ios --target aarch64-apple-ios-sim",
                )
            } else if is_android {
                (
                    "libperry_ui_android.a",
                    "cargo build --release -p perry-ui-android --target aarch64-linux-android",
                )
            } else if is_linux {
                (
                    "libperry_ui_gtk4.a",
                    "cargo build --release -p perry-ui-gtk4 --target x86_64-unknown-linux-gnu",
                )
            } else if is_windows {
                (
                    "perry_ui_windows.lib",
                    "cargo build --release -p perry-ui-windows --target x86_64-pc-windows-msvc",
                )
            } else {
                (
                    "libperry_ui_macos.a",
                    "cargo build --release -p perry-ui-macos",
                )
            };
            return Err(anyhow!(
                "perry/ui imported but {} not found. Build with: {}",
                lib_name,
                build_cmd
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
    let mut seen_swift_sources: std::collections::HashSet<PathBuf> =
        std::collections::HashSet::new();
    for native_lib in &ctx.native_libraries {
        if let Some(ref target_config) = native_lib.target_config {
            match format {
                OutputFormat::Text => {
                    println!("Building native library: {} ...", native_lib.module)
                }
                OutputFormat::Json => {}
            }

            // Issue #860 — prebuilt-distribution shortcut. When the
            // wrapper's manifest specified a `prebuilt:` path that
            // resolved to an on-disk static library, skip the cargo
            // build entirely and link the prebuilt archive directly.
            // `frameworks` / `libs` / `pkgConfig` / `lib_dirs` are
            // still honored below — those are linker flags the host
            // toolchain needs regardless of where the `.a` came from.
            if let Some(prebuilt) = target_config.prebuilt.as_ref() {
                if !prebuilt.exists() {
                    return Err(anyhow!(
                        "Prebuilt native library declared by {} not found at {}. \
                         If this package is distributed via npm `optionalDependencies` \
                         (esbuild/sharp pattern), make sure the per-platform subpackage \
                         is installed for the current host/target.",
                        native_lib.module,
                        prebuilt.display()
                    ));
                }
                cmd.arg(prebuilt);
                match format {
                    OutputFormat::Text => {
                        println!("Linking prebuilt native library: {}", prebuilt.display())
                    }
                    OutputFormat::Json => {}
                }
            } else {
                // Build the Rust crate
                let cargo_toml = target_config.crate_path.join("Cargo.toml");
                if cargo_toml.exists() {
                    // Tier 3 targets (tvOS, watchOS) need nightly + build-std
                    let is_tier3 = matches!(
                        target,
                        Some("tvos")
                            | Some("tvos-simulator")
                            | Some("watchos")
                            | Some("watchos-simulator")
                    );

                    // #505: optionally wrap the cargo invocation in
                    // `sandbox-exec` on macOS to deny network and
                    // restrict FS writes during `build.rs` execution.
                    // Off by default for backwards compat; opted into
                    // via `PERRY_SANDBOX_BUILDRS=1`. Packages listed
                    // in `perry.allowUnsandboxedBuild` are exempt.
                    let mut cargo_cmd =
                        super::sandbox_buildrs::wrap_cargo_command(ctx, &native_lib.module);
                    if is_tier3 {
                        cargo_cmd.arg("+nightly");
                    }
                    cargo_cmd
                        .arg("build")
                        .arg("--release")
                        .arg("--manifest-path")
                        .arg(&cargo_toml);

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
                        cargo_cmd.env(
                            "CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS",
                            "-C link-arg=-Wl,-z,max-page-size=16384",
                        );
                    }

                    // For HarmonyOS, point cargo at the OHOS SDK's clang + sysroot
                    // so cc-rs and rustc's linker invocation actually use the
                    // cross-toolchain instead of falling back to the host `cc`.
                    if is_harmonyos {
                        if let Some(sdk) = super::library_search::find_harmonyos_sdk() {
                            for (k, v) in super::library_search::harmonyos_cross_env(&sdk, target) {
                                cargo_cmd.env(k, v);
                            }
                        }
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
                    // Search in the crate's target directory first, then standard paths.
                    // Refs #564: probe both `target/release/` and
                    // `target/<host-triple>/release/` for native builds — cargo
                    // writes to the triple-prefixed dir when a default target is
                    // pinned via `[build] target` / `CARGO_BUILD_TARGET` /
                    // `rust-toolchain.toml`.
                    let crate_target_dir = target_config.crate_path.join("target");
                    let lib_path = super::library_search::locate_native_lib_artifact(
                        &crate_target_dir,
                        target,
                        lib_name,
                    );

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
                            OutputFormat::Text => {
                                println!("Linking native library: {}", lib.display())
                            }
                            OutputFormat::Json => {}
                        }
                    } else {
                        return Err(anyhow!(
                            "Native library {} not found after building {} crate",
                            lib_name,
                            native_lib.module
                        ));
                    }
                }
            } // closes else of `if let Some(prebuilt) = ...` (issue #860)

            // Add platform frameworks
            for framework in &target_config.frameworks {
                cmd.arg("-framework").arg(framework);
            }

            // Add library search paths. MSVC link.exe takes `/LIBPATH:`;
            // every other linker we drive (clang/ld on Apple, gcc/ld on
            // Linux/Android/HarmonyOS) understands `-L`. Mirror the
            // `target_config.libs` branch immediately below so a
            // `targets.windows.libDirs` entry actually resolves the
            // `{lib}.lib` lookups instead of being a silent no-op.
            for lib_dir in &target_config.lib_dirs {
                if is_windows {
                    cmd.arg(format!("/LIBPATH:{}", lib_dir.display()));
                } else {
                    cmd.arg(format!("-L{}", lib_dir.display()));
                }
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

                let swift_obj_dir =
                    std::env::temp_dir().join(format!("perry_swift_{}", std::process::id()));
                std::fs::create_dir_all(&swift_obj_dir).ok();

                for swift_src in &target_config.swift_sources {
                    if !swift_src.exists() {
                        return Err(anyhow!(
                            "Swift source not found: {} (declared in {}'s nativeLibrary.swift_sources)",
                            swift_src.display(),
                            native_lib.module
                        ));
                    }
                    let canonical = swift_src
                        .canonicalize()
                        .unwrap_or_else(|_| swift_src.clone());
                    if !seen_swift_sources.insert(canonical) {
                        continue;
                    }
                    let stem = swift_src
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("swift_src");
                    let obj_out = swift_obj_dir.join(format!("{}.o", stem));
                    let status = Command::new(&swiftc)
                        .arg("-target")
                        .arg(swift_triple)
                        .arg("-sdk")
                        .arg(&swift_sysroot)
                        .arg("-parse-as-library")
                        .arg("-emit-object")
                        .arg("-O")
                        .arg("-o")
                        .arg(&obj_out)
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
                        OutputFormat::Text => {
                            println!("Linking Swift object: {}", obj_out.display())
                        }
                        OutputFormat::Json => {}
                    }
                }
            }

            // Metal sources are compiled + packed into <app>.app/default.metallib
            // after the `.app` bundle is created below. Just validate the target
            // here so we fail early with a clear message instead of silently
            // dropping shaders on non-Apple-bundle targets.
            if !target_config.metal_sources.is_empty()
                && !matches!(
                    target,
                    Some("ios")
                        | Some("ios-simulator")
                        | Some("tvos")
                        | Some("tvos-simulator")
                        | Some("watchos")
                        | Some("watchos-simulator")
                )
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
