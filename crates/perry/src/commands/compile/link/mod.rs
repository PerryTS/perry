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
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OutputFormat;

use super::{
    apple_sdk_version, build_geisterhand_libs, dedup_native_lib_for_tier3, dedup_runtime_for_tier3,
    dedup_stdlib_for_tier3, find_geisterhand_library, find_geisterhand_runtime,
    find_geisterhand_stdlib, find_geisterhand_ui, find_lld_link, find_llvm_tool,
    find_msvc_lib_paths, find_msvc_link_exe, find_perry_windows_sdk, find_stdlib_library,
    find_ui_library, find_visionos_swift_runtime, find_watchos_swift_runtime,
    localize_stdlib_stub_symbols, localize_stdlib_stub_symbols_for_windows, rust_target_triple,
    strip_bundled_runtime_from_well_known_lib, strip_bundled_shared_deps_from_well_known_lib,
    strip_duplicate_objects_from_lib, strip_duplicate_objects_from_well_known_lib,
    windows_pe_subsystem_flag, windows_subsystem_needs_ui, CompilationContext,
};

mod build_and_run;
mod link_cache;
mod linux_ui_libs;
mod native_features;
mod pkg_config;
mod platform_cmd;
mod windows_link;

pub(super) use build_and_run::build_and_run_link;
use link_cache::prepare_link_cache_status;
pub(super) use link_cache::{write_link_cache_manifest, LinkCacheStatus};
pub use platform_cmd::select_linker_command;
#[cfg(test)]
pub(super) use windows_link::WINDOWS_APP_MANIFEST; // consumed only by windows_link_tests
#[cfg(test)]
pub(super) use windows_link::{embed_app_manifest, linker_is_msvc_link_exe}; // windows_link_tests (#6023)

/// Symbols a plugin host must export so `dlopen`'d / `LoadLibrary`'d plugin
/// shared libraries can resolve them against the host process at load time.
///
/// Plugins (`.dylib` / `.so` / `.dll`) link against the host's copies of
/// these symbols rather than bringing their own — see
/// `crates/perry-runtime/src/plugin.rs::perry_plugin_load`. On macOS we
/// use `-Wl,-u,_<sym>` to force the linker to keep them past dead-strip;
/// on Linux `-rdynamic` exports the whole set; on Windows we write a
/// `.def` file and pass `/DEF:<path>` to `link.exe` (the MSVC equivalent
/// of `-rdynamic`).
pub(super) const PLUGIN_HOST_SYMBOLS: &[&str] = &[
    // Runtime allocation / value primitives
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
    // Plugin manager (perry-plugin)
    "perry_plugin_load",
    "perry_plugin_unload",
    "perry_plugin_emit_hook",
    "perry_plugin_emit_event",
    "perry_plugin_invoke_tool",
    "perry_plugin_register_hook",
    "perry_plugin_register_hook_ex",
    "perry_plugin_unregister_hook",
    "perry_plugin_register_tool",
    "perry_plugin_unregister_tool",
    "perry_plugin_register_service",
    "perry_plugin_unregister_service",
    "perry_plugin_register_route",
    "perry_plugin_unregister_route",
    "perry_plugin_list_plugins",
    "perry_plugin_list_hooks",
    "perry_plugin_list_tools",
    "perry_plugin_count",
    "perry_plugin_set_metadata",
    "perry_plugin_get_config",
    "perry_plugin_on",
    "perry_plugin_off",
    "perry_plugin_emit",
    "perry_plugin_log",
    "perry_plugin_set_config",
    "perry_plugin_discover",
    "perry_plugin_init",
    // Process-wide class-field inline guard (perry-runtime/src/object/mod.rs).
    // A `#[no_mangle] pub static`; not picked up by `llvm-nm` filter heuristics
    // for every codebase, so list it explicitly so the plugin DLL can find it
    // at LoadLibrary time.
    "PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeBackendLinkMetadata {
    backend: super::NativeBackend,
    prebuilt: Option<PathBuf>,
    frameworks: Vec<String>,
    libs: Vec<String>,
    lib_dirs: Vec<PathBuf>,
    pkg_config: Vec<String>,
}

fn select_available_backend_link_metadata(
    target_config: &super::TargetNativeConfig,
) -> Vec<NativeBackendLinkMetadata> {
    if !target_config.available {
        return Vec::new();
    }

    target_config
        .backends
        .iter()
        .filter(|backend| backend.available)
        .map(|backend| NativeBackendLinkMetadata {
            backend: backend.backend,
            prebuilt: backend.prebuilt.clone(),
            frameworks: backend.frameworks.clone(),
            libs: backend.libs.clone(),
            lib_dirs: backend.lib_dirs.clone(),
            pkg_config: backend.pkg_config.clone(),
        })
        .collect()
}

/// Windows system libraries that wgpu's graphics backends reference but
/// the wgpu crate's own `#[link]` attrs don't cover (#5812 item 2):
///
/// - `d3dcompiler.lib` — the d3d12 backend calls `D3DCompile` (HLSL →
///   DXBC shader compilation).
/// - `opengl32.lib` — wgpu's gl backend (compiled in alongside
///   d3d12/vulkan on Windows) calls the `wgl*` context entry points.
///
/// Without these, a Windows app linking a wgpu-backed native ext fails
/// with LNK2019 unresolved externals. Keyed off the manifest's *declared*
/// backends (not the `available` prebuilt flag): the unresolved symbols
/// come from the main staticlib's references, and appending an
/// unreferenced `.lib` is harmless on MSVC (only referenced objects are
/// pulled). d3d12 is Windows-only, so its presence reliably signals the
/// ext pulls wgpu's d3d12 + gl backends; vulkan-on-Windows still compiles
/// the gl fallback, so it needs `opengl32` too. Returns the `.lib` names
/// in link order.
fn windows_wgpu_backend_syslibs(target_config: &super::TargetNativeConfig) -> Vec<&'static str> {
    let has_d3d12 = target_config
        .backends
        .iter()
        .any(|backend| backend.backend.as_str() == "d3d12");
    let has_vulkan = target_config
        .backends
        .iter()
        .any(|backend| backend.backend.as_str() == "vulkan");
    let mut libs = Vec::new();
    if has_d3d12 {
        libs.push("d3dcompiler.lib");
    }
    if has_d3d12 || has_vulkan {
        libs.push("opengl32.lib");
    }
    libs
}

#[cfg(test)]
mod native_package_selection_tests {
    use super::*;
    use crate::commands::compile::{
        NativeBackend, NativeBackendConfig, NativeBackendPackageMetadata, TargetNativeConfig,
    };

    fn target_config() -> TargetNativeConfig {
        TargetNativeConfig {
            available: true,
            unavailable_reason: None,
            crate_path: PathBuf::from("crate"),
            lib_name: "demo".to_string(),
            prebuilt: Some(PathBuf::from("target/libdemo.a")),
            frameworks: vec!["Metal".to_string()],
            optional_frameworks: Vec::new(),
            frameworks_env: None,
            libs: vec!["z".to_string()],
            lib_dirs: vec![PathBuf::from("vendor/lib")],
            pkg_config: vec!["openssl".to_string()],
            resources: vec![PathBuf::from("resources/common.dat")],
            shader_outputs: vec![PathBuf::from("shaders/common.spv")],
            backends: Vec::new(),
            swift_sources: Vec::new(),
            metal_sources: vec![PathBuf::from("shaders/default.metal")],
        }
    }

    fn backend_config(backend: NativeBackend, available: bool) -> NativeBackendConfig {
        NativeBackendConfig {
            backend,
            available,
            unavailable_reason: if available {
                None
            } else {
                Some("not shipped".to_string())
            },
            prebuilt: Some(PathBuf::from(format!("backend/{}.a", backend.as_str()))),
            frameworks: vec![format!("{}Framework", backend.as_str())],
            libs: vec![format!("{}_sys", backend.as_str())],
            lib_dirs: vec![PathBuf::from(format!("vendor/{}/lib", backend.as_str()))],
            pkg_config: vec![format!("{}-pkg", backend.as_str())],
            shader_sources: vec![PathBuf::from(format!("shaders/{}.src", backend.as_str()))],
            shader_outputs: vec![PathBuf::from(format!("shaders/{}.bin", backend.as_str()))],
            resources: vec![PathBuf::from(format!("resources/{}", backend.as_str()))],
            package: NativeBackendPackageMetadata {
                name: Some(format!("demo-{}", backend.as_str())),
                version: Some("1.0.0".to_string()),
                kind: Some("shader-package".to_string()),
            },
        }
    }

    #[test]
    fn selection_includes_available_backend_link_metadata() {
        let mut tc = target_config();
        tc.backends
            .push(backend_config(NativeBackend::Vulkan, true));
        tc.backends
            .push(backend_config(NativeBackend::D3d12, false));

        let selection = select_available_backend_link_metadata(&tc);

        assert_eq!(selection.len(), 1);
        let backend = &selection[0];
        assert_eq!(backend.backend, NativeBackend::Vulkan);
        assert_eq!(backend.prebuilt, Some(PathBuf::from("backend/vulkan.a")));
        assert_eq!(backend.frameworks, vec!["vulkanFramework".to_string()]);
        assert_eq!(backend.libs, vec!["vulkan_sys".to_string()]);
        assert_eq!(backend.lib_dirs, vec![PathBuf::from("vendor/vulkan/lib")]);
        assert_eq!(backend.pkg_config, vec!["vulkan-pkg".to_string()]);
    }

    #[test]
    fn unavailable_target_selects_no_backend_link_metadata() {
        let mut tc = target_config();
        tc.available = false;
        tc.backends
            .push(backend_config(NativeBackend::Vulkan, true));

        let selection = select_available_backend_link_metadata(&tc);

        assert!(selection.is_empty());
    }

    /// #5812 item 2 — a d3d12 backend pulls both `D3DCompile`
    /// (d3dcompiler.lib) and wgpu's gl fallback (opengl32.lib).
    #[test]
    fn d3d12_backend_appends_d3dcompiler_and_opengl32() {
        let mut tc = target_config();
        tc.backends.push(backend_config(NativeBackend::D3d12, true));
        assert_eq!(
            super::windows_wgpu_backend_syslibs(&tc),
            vec!["d3dcompiler.lib", "opengl32.lib"]
        );
    }

    /// vulkan-on-Windows still compiles wgpu's gl fallback (opengl32.lib)
    /// but does not need d3dcompiler.
    #[test]
    fn vulkan_backend_appends_only_opengl32() {
        let mut tc = target_config();
        tc.backends
            .push(backend_config(NativeBackend::Vulkan, true));
        assert_eq!(
            super::windows_wgpu_backend_syslibs(&tc),
            vec!["opengl32.lib"]
        );
    }

    /// The syslibs are keyed off *declared* backends, not the `available`
    /// prebuilt flag — the unresolved symbols come from the main staticlib,
    /// and an unreferenced `.lib` is harmless on MSVC.
    #[test]
    fn d3d12_backend_appends_syslibs_even_when_unavailable() {
        let mut tc = target_config();
        tc.backends
            .push(backend_config(NativeBackend::D3d12, false));
        assert_eq!(
            super::windows_wgpu_backend_syslibs(&tc),
            vec!["d3dcompiler.lib", "opengl32.lib"]
        );
    }

    /// A non-graphics native lib (no wgpu backends) gets no extra syslibs.
    #[test]
    fn no_backends_appends_no_syslibs() {
        let tc = target_config();
        assert!(super::windows_wgpu_backend_syslibs(&tc).is_empty());
    }

    /// Metal is Apple-only and never triggers the Windows wgpu syslibs.
    #[test]
    fn metal_backend_appends_no_windows_syslibs() {
        let mut tc = target_config();
        tc.backends.push(backend_config(NativeBackend::Metal, true));
        assert!(super::windows_wgpu_backend_syslibs(&tc).is_empty());
    }
}

/// Walk up from the entry `.ts` to the directory holding `perry.toml`.
/// Mirrors `widget_build::project_root_for` — kept local so the link
/// module doesn't reach across sibling modules for one path lookup.
fn find_project_root_for(input: &Path) -> Option<PathBuf> {
    let mut dir = input.canonicalize().ok()?;
    for _ in 0..8 {
        dir = dir.parent()?.to_path_buf();
        if dir.join("perry.toml").exists() {
            return Some(dir);
        }
    }
    None
}

/// Resolve the vendored optional-framework search directory for a native
/// library that gates an SDK on `frameworks_env` (issue #1303 — e.g.
/// `@perryts/google-auth`'s `PERRY_GOOGLE_SIGN_IN_FRAMEWORK_DIR`).
///
/// Precedence (matches the issue's contract):
///   1. The `frameworks_env` env var, when set in the process environment
///      — today's local `perry compile` behavior. Returned verbatim.
///   2. `perry.toml [google_auth].framework_dir`, resolved relative to the
///      project root → absolute. This is what survives the `perry publish`
///      worker round-trip: the dev's shell env doesn't transfer and an
///      absolute local path wouldn't exist on the worker, but perry.toml +
///      the project-relative dir are both uploaded with `--project`, so the
///      worker's `perry compile` re-resolves the same dir.
///
/// Returns `None` when neither source yields a path. Callers still check
/// `is_dir()` before linking, so a stale/misspelled path skips silently
/// (the wrapper's `#if canImport(...)` fallback keeps the link valid).
fn resolve_optional_framework_dir(env_name: &str, args_input: &Path) -> Option<PathBuf> {
    // 1. Explicit env var wins (today's behavior).
    if let Some(dir) = std::env::var_os(env_name) {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    // 2. Project-relative `perry.toml [google_auth].framework_dir`.
    let project_root = find_project_root_for(args_input)?;
    let content = fs::read_to_string(project_root.join("perry.toml")).ok()?;
    let doc = content.parse::<toml::Table>().ok()?;
    let rel = doc
        .get("google_auth")?
        .as_table()?
        .get("framework_dir")?
        .as_str()?;
    Some(project_root.join(rel))
}

/// Quote one linker argument for a response file. `msvc` selects `link.exe` /
/// `lld-link` rules (a `"` toggles a quoted run; backslashes are literal Windows
/// path separators, so they are NOT escaped — only an embedded `"` is escaped as
/// `\"`); otherwise GNU/clang rules (inside double quotes `\` and `"` escape, so
/// both are backslash-escaped). An argument with no whitespace/quote needs no
/// quoting in either dialect — the common case (long object/lib paths).
pub(super) fn quote_response_arg(arg: &str, msvc: bool) -> String {
    let needs_quote = arg.is_empty()
        || arg
            .bytes()
            .any(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'"'));
    if !needs_quote {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for c in arg.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' if !msvc => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a linker `Command`'s full argument vector as response-file content
/// (one quoted arg per line). Pure + unit-tested; the `\n` separator and
/// per-arg quoting are what `link.exe`/`lld-link`/`clang` accept via `@file`.
pub(super) fn response_file_contents(args: &[String], msvc: bool) -> String {
    let mut s = String::new();
    for a in args {
        s.push_str(&quote_response_arg(a, msvc));
        s.push('\n');
    }
    s
}

/// Rewrite a linker invocation to pass its arguments via a response file
/// (`<linker> @<file>`) instead of inline, dodging the Windows `CreateProcess`
/// command-line length cap (os error 206) on links with many object files.
/// Preserves program, env overrides, and working directory. The response file
/// goes in the OS temp dir under a per-process/-output unique name (so parallel
/// links don't clobber each other) and the caller deletes it after the link.
/// Returns `None` if there's nothing to gain (no args) or the file can't be
/// written — the caller then keeps the original inline command.
fn rewrite_link_with_response_file(cmd: &Command, msvc: bool) -> Option<(Command, PathBuf)> {
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if args.is_empty() {
        return None;
    }
    let contents = response_file_contents(&args, msvc);
    let rsp = std::env::temp_dir().join(format!("perry-link-{}.rsp", std::process::id()));
    fs::write(&rsp, contents).ok()?;

    let mut new_cmd = Command::new(cmd.get_program());
    new_cmd.arg(format!("@{}", rsp.display()));
    // Preserve env overrides (e.g. MSVC LIB/PATH set by select_linker_command)
    // and the working directory the original command was configured with.
    for (key, val) in cmd.get_envs() {
        match val {
            Some(v) => {
                new_cmd.env(key, v);
            }
            None => {
                new_cmd.env_remove(key);
            }
        }
    }
    if let Some(cwd) = cmd.get_current_dir() {
        new_cmd.current_dir(cwd);
    }
    Some((new_cmd, rsp))
}

#[cfg(test)]
mod optional_framework_dir_tests;

#[cfg(test)]
mod response_file_tests {
    use super::{quote_response_arg, response_file_contents};

    #[test]
    fn plain_paths_are_unquoted_in_both_dialects() {
        assert_eq!(quote_response_arg("/tmp/a_ts.o", false), "/tmp/a_ts.o");
        assert_eq!(
            quote_response_arg(r"C:\build\d0449_ts.o", true),
            r"C:\build\d0449_ts.o"
        );
        // /OPT:REF etc. — no whitespace, untouched.
        assert_eq!(quote_response_arg("/OPT:REF", true), "/OPT:REF");
    }

    #[test]
    fn msvc_quotes_spaces_keeps_backslashes_literal() {
        // Windows path with a space: quoted, backslashes NOT escaped.
        assert_eq!(
            quote_response_arg(r"C:\Program Files\x.lib", true),
            "\"C:\\Program Files\\x.lib\""
        );
    }

    #[test]
    fn gnu_quotes_and_escapes_backslashes() {
        assert_eq!(quote_response_arg("/a b/x.o", false), "\"/a b/x.o\"");
        assert_eq!(
            quote_response_arg(r"/a\b c/x.o", false),
            "\"/a\\\\b c/x.o\""
        );
    }

    #[test]
    fn embedded_quote_is_escaped() {
        assert_eq!(quote_response_arg("a\"b", true), "\"a\\\"b\"");
        assert_eq!(quote_response_arg("a\"b", false), "\"a\\\"b\"");
    }

    #[test]
    fn contents_is_one_arg_per_line() {
        let args = vec![
            "/tmp/main.o".to_string(),
            "/tmp/mod 1.o".to_string(),
            "-lperry".to_string(),
        ];
        assert_eq!(
            response_file_contents(&args, false),
            "/tmp/main.o\n\"/tmp/mod 1.o\"\n-lperry\n"
        );
    }
}
