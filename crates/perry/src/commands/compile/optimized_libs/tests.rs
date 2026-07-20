use super::*;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::commands::stdlib_features::{compute_required_features, features_to_cargo_arg};
use crate::OutputFormat;

use super::super::{find_perry_workspace_root, rust_target_triple, CompilationContext};

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env lock poisoned")
}

fn set_env_var(key: &str, value: Option<&str>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, contents).expect("write test file");
}

fn minimal_auto_workspace(dir: &Path) {
    write_file(&dir.join("Cargo.toml"), b"[workspace]\n");
    write_file(&dir.join("Cargo.lock"), b"# lock\n");
    write_file(&dir.join("crates/perry-runtime/Cargo.toml"), b"[package]\n");
    write_file(
        &dir.join("crates/perry-runtime/src/lib.rs"),
        b"pub fn rt() {}\n",
    );
    write_file(&dir.join("crates/perry-stdlib/Cargo.toml"), b"[package]\n");
    write_file(
        &dir.join("crates/perry-stdlib/src/lib.rs"),
        b"pub fn stdlib() {}\n",
    );
}

#[test]
fn auto_optimized_archives_are_fresh_when_newer_than_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    minimal_auto_workspace(dir.path());
    std::thread::sleep(std::time::Duration::from_millis(10));

    let runtime = dir
        .path()
        .join("target/perry-auto/release/libperry_runtime.a");
    let stdlib = dir
        .path()
        .join("target/perry-auto/release/libperry_stdlib.a");
    write_file(&runtime, b"!<arch>\n");
    write_file(&stdlib, b"!<arch>\n");
    let stamp = dir.path().join("target/perry-auto/.perry-auto-build.stamp");
    write_file(&stamp, b"test-stamp");

    assert!(auto_optimized_archives_are_fresh(
        dir.path(),
        &runtime,
        &stdlib,
        &[],
        &stamp,
        "test-stamp"
    ));
}

#[test]
fn build_optimized_libs_reuses_fresh_auto_archives_without_cargo() {
    let _env = env_lock();
    let original_path = std::env::var_os("PATH");
    let original_bitcode = std::env::var_os("PERRY_LLVM_BITCODE_LINK");
    let workspace_root = find_perry_workspace_root().expect("workspace root");

    let mut ctx = CompilationContext::new(workspace_root.clone());
    ctx.needs_wasm_runtime = true;

    // Derive the cache key / target dir / stamp exactly as
    // `build_optimized_libs` does for this ctx, so the freshness probe finds
    // the archives we plant (instead of hardcoding a key string that drifts
    // whenever the cache-key inputs change).
    // Mirror build_optimized_libs's feature derivation for this import-free
    // ctx: it always force-adds `crypto` (perry-stdlib's crypto module is
    // unconditionally linked into the auto-optimize rebuild), and the
    // import-/fetch-driven unions don't fire for a fresh ctx.
    let mut features = compute_required_features(
        &ctx.native_module_imports,
        ctx.uses_fetch,
        ctx.uses_crypto_builtins,
    );
    features.insert("crypto");
    let feature_arg = features_to_cargo_arg(&features);
    let panic_abort_safe =
        !ctx.needs_ui && !ctx.needs_thread && !ctx.needs_plugins && !ctx.needs_geisterhand;
    let key_input = auto_optimized_cache_key(&feature_arg, panic_abort_safe, None, &ctx);
    let mut hash: u64 = 5381;
    for b in key_input.as_bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(*b as u64);
    }
    let (target_dir, _) = auto_target_dir_paths(&workspace_root, hash);
    let release_dir = target_dir.join("release");
    let runtime = release_dir.join("libperry_runtime.a");
    let stdlib = release_dir.join("libperry_stdlib.a");
    std::fs::create_dir_all(&release_dir).expect("mkdir release dir");
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_file(&runtime, b"!<arch>\n");
    write_file(&stdlib, b"!<arch>\n");
    let cross_features = auto_optimized_cross_features(&ctx, &features, &[]);
    let source_fingerprint = auto_optimized_source_fingerprint(&workspace_root, &[]);
    let stamp =
        auto_optimized_build_stamp(&key_input, None, &cross_features, &[], &source_fingerprint);
    write_file(
        &target_dir.join(".perry-auto-build.stamp"),
        stamp.as_bytes(),
    );

    let fake_path = tempfile::tempdir().expect("fake PATH");
    std::env::set_var("PATH", fake_path.path());
    std::env::remove_var("PERRY_LLVM_BITCODE_LINK");

    let libs = build_optimized_libs(&ctx, None, &[], OutputFormat::Json, 0);

    set_env_var("PATH", original_path.as_deref().and_then(|v| v.to_str()));
    set_env_var(
        "PERRY_LLVM_BITCODE_LINK",
        original_bitcode.as_deref().and_then(|v| v.to_str()),
    );

    assert_eq!(libs.runtime.as_deref(), Some(runtime.as_path()));
    assert_eq!(libs.stdlib.as_deref(), Some(stdlib.as_path()));
}

#[test]
fn auto_optimized_archives_are_stale_when_runtime_source_is_newer() {
    let dir = tempfile::tempdir().expect("tempdir");
    minimal_auto_workspace(dir.path());
    let runtime = dir
        .path()
        .join("target/perry-auto/release/libperry_runtime.a");
    let stdlib = dir
        .path()
        .join("target/perry-auto/release/libperry_stdlib.a");
    write_file(&runtime, b"!<arch>\n");
    write_file(&stdlib, b"!<arch>\n");
    let stamp = dir.path().join("target/perry-auto/.perry-auto-build.stamp");
    write_file(&stamp, b"test-stamp");
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_file(
        &dir.path().join("crates/perry-runtime/src/lib.rs"),
        b"pub fn rt_changed() {}\n",
    );

    assert!(!auto_optimized_archives_are_fresh(
        dir.path(),
        &runtime,
        &stdlib,
        &[],
        &stamp,
        "test-stamp"
    ));
}

#[test]
fn auto_optimized_freshness_ignores_nested_target_dirs() {
    let dir = tempfile::tempdir().expect("tempdir");
    minimal_auto_workspace(dir.path());
    std::thread::sleep(std::time::Duration::from_millis(10));
    let runtime = dir
        .path()
        .join("target/perry-auto/release/libperry_runtime.a");
    let stdlib = dir
        .path()
        .join("target/perry-auto/release/libperry_stdlib.a");
    write_file(&runtime, b"!<arch>\n");
    write_file(&stdlib, b"!<arch>\n");
    let stamp = dir.path().join("target/perry-auto/.perry-auto-build.stamp");
    write_file(&stamp, b"test-stamp");
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_file(
        &dir.path()
            .join("crates/perry-runtime/target/debug/stale-marker"),
        b"newer but irrelevant\n",
    );

    assert!(auto_optimized_archives_are_fresh(
        dir.path(),
        &runtime,
        &stdlib,
        &[],
        &stamp,
        "test-stamp"
    ));
}

/// #5892 layer 2 / #5778 warm-cache trap: the auto-opt freshness gate must be
/// keyed on the CONTENT of every source tree that lands in the archives — an
/// ext-crate edit must rotate the fingerprint even when mtimes lie (cache
/// restores, fresh checkouts), and rewriting identical bytes must NOT.
#[test]
fn source_fingerprint_tracks_ext_crate_content_not_mtimes() {
    let dir = tempfile::tempdir().expect("tempdir");
    minimal_auto_workspace(dir.path());
    write_file(
        &dir.path().join("crates/perry-ext-http/Cargo.toml"),
        b"[package]\n",
    );
    write_file(
        &dir.path().join("crates/perry-ext-http/src/lib.rs"),
        b"pub fn http() {}\n",
    );
    let bindings = vec![(
        "perry-ext-http".to_string(),
        "perry_ext_http".to_string(),
        None,
    )];

    let fp1 = auto_optimized_source_fingerprint(dir.path(), &bindings);

    // Rewriting identical bytes (mtime-only churn) must not rotate the key.
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_file(
        &dir.path().join("crates/perry-ext-http/src/lib.rs"),
        b"pub fn http() {}\n",
    );
    assert_eq!(
        fp1,
        auto_optimized_source_fingerprint(dir.path(), &bindings)
    );

    // A content edit in the ext crate must rotate it — this is exactly the
    // stale-archive reuse that masked #5911 in CI.
    write_file(
        &dir.path().join("crates/perry-ext-http/src/lib.rs"),
        b"pub fn http_changed() {}\n",
    );
    let fp2 = auto_optimized_source_fingerprint(dir.path(), &bindings);
    assert_ne!(fp1, fp2);

    // A binding crate that isn't routed must not affect the key.
    assert_ne!(
        auto_optimized_source_fingerprint(dir.path(), &[]),
        fp2,
        "fingerprint should include routed binding crates"
    );
}

/// The fingerprint must follow transitive workspace path-deps: an edit in a
/// crate reachable only through another crate's manifest (here perry-ext-net
/// via perry-ext-http) still lands in the archives, so it must rotate the key.
#[test]
fn source_fingerprint_follows_workspace_dep_closure() {
    let dir = tempfile::tempdir().expect("tempdir");
    minimal_auto_workspace(dir.path());
    write_file(
        &dir.path().join("crates/perry-ext-http/Cargo.toml"),
        b"[package]\n[dependencies]\nperry-ext-net = { path = \"../perry-ext-net\" }\n",
    );
    write_file(
        &dir.path().join("crates/perry-ext-http/src/lib.rs"),
        b"pub fn http() {}\n",
    );
    write_file(
        &dir.path().join("crates/perry-ext-net/Cargo.toml"),
        b"[package]\n",
    );
    write_file(
        &dir.path().join("crates/perry-ext-net/src/lib.rs"),
        b"pub fn net() {}\n",
    );
    let bindings = vec![(
        "perry-ext-http".to_string(),
        "perry_ext_http".to_string(),
        None,
    )];

    let fp1 = auto_optimized_source_fingerprint(dir.path(), &bindings);
    write_file(
        &dir.path().join("crates/perry-ext-net/src/lib.rs"),
        b"pub fn net_changed() {}\n",
    );
    assert_ne!(
        fp1,
        auto_optimized_source_fingerprint(dir.path(), &bindings)
    );
}

/// Closes #507. The well-known flip's "shared tokio" allowlist
/// must match the set of perry-ext-* crates whose own
/// `Cargo.toml` pulls tokio. If a new wrapper is added that uses
/// tokio for I/O without being added here, programs importing it
/// will panic with "there is no reactor running" the first time
/// the wrapper calls `Handle::current()` on a tokio worker.
#[test]
fn net_needs_shared_tokio() {
    assert!(binding_needs_shared_tokio("net"));
}

#[test]
fn cpu_only_wrappers_do_not_need_shared_tokio() {
    // bcrypt / argon2 / sharp / dotenv all route through
    // perry-stdlib's `spawn_blocking` shim; their own crate has
    // no tokio dep, so there's no CONTEXT collision risk.
    assert!(!binding_needs_shared_tokio("bcrypt"));
    assert!(!binding_needs_shared_tokio("argon2"));
    assert!(!binding_needs_shared_tokio("sharp"));
    assert!(!binding_needs_shared_tokio("dotenv"));
}

#[test]
fn unknown_modules_default_to_workspace_path() {
    // Defensive default: if a module isn't in the allowlist,
    // treat it as CPU-only (existing v0.5.586 behavior).
    assert!(!binding_needs_shared_tokio("definitely-not-a-real-package"));
}

#[test]
fn builtin_fetch_usage_does_not_synthesize_well_known_fetch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.uses_fetch = true;

    let modules = well_known_iteration_set(&ctx);

    assert!(
        !modules.contains("fetch"),
        "built-in Web Fetch should stay on perry-stdlib so erased-type dispatch shares the constructor registry"
    );
}

#[test]
fn explicit_node_fetch_import_still_routes_to_well_known_fetch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.native_module_imports.insert("node-fetch".to_string());

    let modules = well_known_iteration_set(&ctx);

    assert!(modules.contains("node-fetch"));
}

#[test]
fn http2_import_enables_http2_constants_cross_feature() {
    // #6468: importing `node:http2` records "http2" in `native_module_imports`,
    // which must flip on `perry-runtime/mod-http2-constants` so the constant
    // tables (`node_http2_constants`) are linked. A program that never imports
    // it must leave the feature off so the tables dead-strip.
    let dir = tempfile::tempdir().expect("tempdir");
    let empty_features: std::collections::BTreeSet<&'static str> =
        std::collections::BTreeSet::new();

    let mut with_http2 = CompilationContext::new(dir.path().to_path_buf());
    with_http2.native_module_imports.insert("http2".to_string());
    let cross_on = auto_optimized_cross_features(&with_http2, &empty_features, &[]);
    assert!(
        cross_on
            .iter()
            .any(|f| f == "perry-runtime/mod-http2-constants"),
        "http2 import should enable mod-http2-constants, got {cross_on:?}"
    );

    let without = CompilationContext::new(dir.path().to_path_buf());
    let cross_off = auto_optimized_cross_features(&without, &empty_features, &[]);
    assert!(
        !cross_off
            .iter()
            .any(|f| f == "perry-runtime/mod-http2-constants"),
        "no http2 import should leave mod-http2-constants off, got {cross_off:?}"
    );
}

#[test]
fn http2_import_changes_optimized_libs_cache_key() {
    // #6468: the http2-constants usage bit participates in the auto-build cache
    // key, so a runtime built without the constant tables is never reused for a
    // program that imports `node:http2`.
    let dir = tempfile::tempdir().expect("tempdir");

    let base = CompilationContext::new(dir.path().to_path_buf());
    let key_without = auto_optimized_cache_key("", true, None, &base);

    let mut with_http2 = CompilationContext::new(dir.path().to_path_buf());
    with_http2.native_module_imports.insert("http2".to_string());
    let key_with = auto_optimized_cache_key("", true, None, &with_http2);

    assert_ne!(
        key_without, key_with,
        "an http2 import must change the auto-optimized cache key"
    );
}

#[test]
fn forced_well_known_env_extends_iteration_set() {
    let _guard = env_lock();
    let old_force_well_known = std::env::var("PERRY_FORCE_WELL_KNOWN").ok();

    set_env_var(
        "PERRY_FORCE_WELL_KNOWN",
        Some("http, node:net ws definitely-not-real"),
    );
    let ctx = CompilationContext::new(std::env::current_dir().expect("cwd"));
    let modules = well_known_iteration_set(&ctx);

    set_env_var("PERRY_FORCE_WELL_KNOWN", old_force_well_known.as_deref());

    assert!(modules.contains("http"));
    assert!(modules.contains("net"));
    assert!(modules.contains("ws"));
    assert!(!modules.contains("node:net"));
    assert!(!modules.contains("definitely-not-real"));
}

#[test]
fn no_auto_still_resolves_prebuilt_well_known_archives() {
    let _guard = env_lock();
    let old_lib_dir = std::env::var("PERRY_LIB_DIR").ok();
    let old_runtime_dir = std::env::var("PERRY_RUNTIME_DIR").ok();
    let old_disable_well_known = std::env::var("PERRY_DISABLE_WELL_KNOWN").ok();

    let dir = tempfile::tempdir().expect("tempdir");
    let http =
        super::super::well_known::lookup_well_known("http").expect("http well-known binding");
    let net = super::super::well_known::lookup_well_known("net").expect("net well-known binding");
    let ws = super::super::well_known::lookup_well_known("ws").expect("ws well-known binding");
    let http_lib = dir
        .path()
        .join(super::super::well_known::ext_staticlib_filename(
            &http.lib,
            rust_target_triple(None),
        ));
    let net_lib = dir
        .path()
        .join(super::super::well_known::ext_staticlib_filename(
            &net.lib,
            rust_target_triple(None),
        ));
    let ws_lib = dir
        .path()
        .join(super::super::well_known::ext_staticlib_filename(
            &ws.lib,
            rust_target_triple(None),
        ));
    std::fs::write(&http_lib, b"!<arch>\n").expect("write fake http archive");
    std::fs::write(&net_lib, b"!<arch>\n").expect("write fake net archive");
    std::fs::write(&ws_lib, b"!<arch>\n").expect("write fake ws archive");

    set_env_var(
        "PERRY_LIB_DIR",
        Some(dir.path().to_str().expect("utf8 temp path")),
    );
    set_env_var("PERRY_RUNTIME_DIR", None);
    set_env_var("PERRY_DISABLE_WELL_KNOWN", None);

    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.native_module_imports.insert("http".to_string());
    ctx.native_module_imports.insert("net".to_string());
    ctx.native_module_imports.insert("ws".to_string());
    let libs = resolve_no_auto_optimized_libs(&ctx, None, OutputFormat::Json, 0);

    set_env_var("PERRY_LIB_DIR", old_lib_dir.as_deref());
    set_env_var("PERRY_RUNTIME_DIR", old_runtime_dir.as_deref());
    set_env_var(
        "PERRY_DISABLE_WELL_KNOWN",
        old_disable_well_known.as_deref(),
    );

    assert_eq!(libs.runtime, None);
    assert_eq!(libs.stdlib, None);
    assert!(
        libs.well_known_libs.contains(&http_lib),
        "expected no-auto well-known libs to include {http_lib:?}, got {:?}",
        libs.well_known_libs
    );
    assert!(
        libs.well_known_libs.contains(&net_lib),
        "expected no-auto well-known libs to include {net_lib:?}, got {:?}",
        libs.well_known_libs
    );
    assert!(
        libs.well_known_libs.contains(&ws_lib),
        "expected no-auto well-known libs to include {ws_lib:?}, got {:?}",
        libs.well_known_libs
    );
}

#[cfg(windows)]
#[test]
fn cargo_target_dir_strips_windows_verbatim_prefixes() {
    let drive = cargo_target_dir_path(PathBuf::from(
        r"\\?\D:\Projects\perry\target\perry-auto-deadbeef",
    ));
    assert_eq!(
        drive,
        PathBuf::from(r"D:\Projects\perry\target\perry-auto-deadbeef")
    );

    let unc = cargo_target_dir_path(PathBuf::from(
        r"\\?\UNC\server\share\perry\target\perry-auto-deadbeef",
    ));
    assert_eq!(
        unc,
        PathBuf::from(r"\\server\share\perry\target\perry-auto-deadbeef")
    );
}

#[cfg(windows)]
#[test]
fn auto_target_dir_uses_relative_cargo_env_path_on_windows() {
    let workspace = PathBuf::from(r"\\?\D:\Projects\perry");
    let (target_dir, cargo_env_dir) = auto_target_dir_paths(&workspace, 0xdeadbeef);

    assert!(
        !cargo_env_dir.is_absolute(),
        "CARGO_TARGET_DIR should stay relative so Cargo build scripts do not receive verbatim Windows paths"
    );
    assert_eq!(
        cargo_env_dir,
        PathBuf::from("target").join("perry-auto-00000000deadbeef")
    );
    assert_eq!(
        target_dir,
        PathBuf::from(r"D:\Projects\perry\target\perry-auto-00000000deadbeef")
    );
}

#[cfg(not(windows))]
#[test]
fn auto_target_dir_keeps_absolute_cargo_env_path_off_windows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (target_dir, cargo_env_dir) = auto_target_dir_paths(dir.path(), 0xdeadbeef);

    assert!(
        cargo_env_dir.is_absolute(),
        "non-Windows hosts should keep the previous absolute CARGO_TARGET_DIR behavior"
    );
    assert_eq!(target_dir, cargo_env_dir);
}

#[cfg(unix)]
#[test]
fn no_auto_builds_missing_well_known_archive_from_workspace_source() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = env_lock();
    let old_path = std::env::var_os("PATH");
    let old_cargo_target_dir = std::env::var_os("CARGO_TARGET_DIR");

    let workspace = tempfile::tempdir().expect("tempdir");
    for dir in [
        "crates/perry-runtime",
        "crates/perry-ui-geisterhand",
        "crates/perry-ext-http",
    ] {
        std::fs::create_dir_all(workspace.path().join(dir)).expect("mkdir workspace marker");
    }

    let fake_bin = workspace.path().join("fake-bin");
    std::fs::create_dir_all(&fake_bin).expect("mkdir fake bin");
    let fake_cargo = fake_bin.join("cargo");
    std::fs::write(
        &fake_cargo,
        r#"#!/bin/sh
case "$*" in
  *"-p perry-ext-http"*) ;;
  *) exit 43 ;;
esac
mkdir -p "$CARGO_TARGET_DIR/release"
printf '!<arch>\n' > "$CARGO_TARGET_DIR/release/libperry_ext_http.a"
"#,
    )
    .expect("write fake cargo");
    let mut perms = std::fs::metadata(&fake_cargo)
        .expect("fake cargo metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&fake_cargo, perms).expect("chmod fake cargo");

    let target_dir = workspace.path().join("out-target");
    let test_path = match old_path.as_ref() {
        Some(path) => {
            let mut paths = vec![fake_bin.clone()];
            paths.extend(std::env::split_paths(path));
            std::env::join_paths(paths).expect("join PATH")
        }
        None => fake_bin.clone().into_os_string(),
    };
    std::env::set_var("PATH", test_path);
    std::env::set_var("CARGO_TARGET_DIR", &target_dir);

    let binding =
        super::super::well_known::lookup_well_known("http").expect("http well-known binding");
    let filename =
        super::super::well_known::ext_staticlib_filename(&binding.lib, rust_target_triple(None));
    let got = build_missing_prebuilt_ext_lib(
        workspace.path(),
        binding,
        &filename,
        None,
        OutputFormat::Json,
        0,
    );

    if let Some(path) = old_path {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
    if let Some(dir) = old_cargo_target_dir {
        std::env::set_var("CARGO_TARGET_DIR", dir);
    } else {
        std::env::remove_var("CARGO_TARGET_DIR");
    }

    assert_eq!(
        got.expect("missing archive should be built from workspace source"),
        target_dir.join("release/libperry_ext_http.a")
    );
}
