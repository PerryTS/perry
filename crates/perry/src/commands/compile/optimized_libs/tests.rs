use super::no_auto::build_missing_prebuilt_ext_lib;
use super::*;
use std::path::Path;

use crate::commands::stdlib_features::{compute_required_features, features_to_cargo_arg};
use crate::OutputFormat;

use super::super::{find_perry_workspace_root, rust_target_triple, CompilationContext};

// The env guard now lives at crate root so tests OUTSIDE this module — which
// read `PATH` while these tests swap it — can take the same lock.
use crate::test_env_lock::env_lock;

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
    // ctx: since the stdlib cherry-pick, `crypto` is no longer force-added
    // (it only joins via imports, `uses_crypto_builtins`, or the codegen
    // `js_crypto_*` prefix net); only the `async-runtime` floor (required
    // by the always-on worker_threads/readline async bridge) is forced, and
    // the import-/fetch-driven unions don't fire for a fresh ctx.
    let mut features = compute_required_features(
        &ctx.native_module_imports,
        ctx.uses_fetch,
        ctx.uses_crypto_builtins,
    );
    features.insert("async-runtime");
    let feature_arg = features_to_cargo_arg(&features);
    let panic_abort_safe =
        !ctx.needs_ui && !ctx.needs_thread && !ctx.needs_plugins && !ctx.needs_geisterhand;
    let panic_immediate = effective_size_panic_immediate_abort(panic_abort_safe);
    let key_input =
        auto_optimized_cache_key(&feature_arg, panic_abort_safe, panic_immediate, None, &ctx);
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

/// The GC-iteration "stale runtime" trap: perry-runtime / perry-stdlib are the
/// crates a runtime dev edits most, yet the pre-existing fingerprint coverage
/// only exercised the routed ext crates. A CONTENT edit to a runtime/stdlib
/// source file must rotate the fingerprint even when the file's mtime does NOT
/// advance (a `git checkout`, `cp -p`, or cache restore can hand back a fresh
/// checkout whose sources look "older" than a cached archive) — the #5892/#5930
/// content fingerprint is exactly what makes that safe, where the mtime gate
/// alone is blind. Rewriting identical bytes must NOT rotate it, so the common
/// no-edit rebuild stays a fast cache hit.
#[test]
fn source_fingerprint_tracks_runtime_and_stdlib_content_not_mtimes() {
    let dir = tempfile::tempdir().expect("tempdir");
    minimal_auto_workspace(dir.path());

    let fp0 = auto_optimized_source_fingerprint(dir.path(), &[]);

    // mtime-only churn (identical bytes, later write time) must not rotate the
    // key — this is the no-edit fast path the freshness gate relies on.
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_file(
        &dir.path().join("crates/perry-runtime/src/lib.rs"),
        b"pub fn rt() {}\n",
    );
    assert_eq!(
        fp0,
        auto_optimized_source_fingerprint(dir.path(), &[]),
        "rewriting identical runtime bytes must not rotate the fingerprint"
    );

    // A content edit to an EXISTING perry-runtime source file must rotate it.
    // Editing in place (rather than adding a new file) is what makes this
    // assertion depend on the file CONTENT hash: a fingerprint that hashed only
    // the path set would still rotate on an added file, and the test could not
    // fail. See `runtime_source_edit_rotates_build_stamp_and_fails_freshness`
    // for the same reasoning applied to the freshness gate.
    write_file(
        &dir.path().join("crates/perry-runtime/src/lib.rs"),
        b"pub fn rt_changed() {}\n",
    );
    let fp_rt = auto_optimized_source_fingerprint(dir.path(), &[]);
    assert_ne!(
        fp0, fp_rt,
        "a perry-runtime source edit must rotate the fingerprint"
    );

    // Same guarantee for perry-stdlib.
    write_file(
        &dir.path().join("crates/perry-stdlib/src/lib.rs"),
        b"pub fn stdlib_changed() {}\n",
    );
    let fp_std = auto_optimized_source_fingerprint(dir.path(), &[]);
    assert_ne!(
        fp_rt, fp_std,
        "a perry-stdlib source edit must rotate the fingerprint"
    );
}

/// Ties the runtime-source fingerprint to the freshness gate the compile driver
/// actually consults: a content edit to perry-runtime rotates the build stamp,
/// so a `target/perry-auto-<hash>` dir stamped for the OLD source can never pass
/// `auto_optimized_archives_are_fresh` — no manual `rm libperry_runtime.a`.
///
/// The stamp gate is the ONLY thing this test lets answer "stale": the archives
/// are planted *after* the source edit, so every source is older than them and
/// the mtime half of `auto_optimized_archives_are_fresh` votes "fresh". That
/// ordering is deliberate — plant them first and the edit's own mtime makes the
/// gate reject on the mtime path alone, so the assertion would still pass with
/// the stamp comparison deleted and the test could never fail (the repo's
/// "a gate must assert its subject was live" rule). It is also the real-world
/// case the content fingerprint exists for: a `git checkout` / `cp -p` / CI
/// cache restore hands back sources whose mtimes never advance past a cached
/// archive.
#[test]
fn runtime_source_edit_rotates_build_stamp_and_fails_freshness() {
    let dir = tempfile::tempdir().expect("tempdir");
    minimal_auto_workspace(dir.path());

    let fp_before = auto_optimized_source_fingerprint(dir.path(), &[]);
    let stamp_before = auto_optimized_build_stamp("key", None, &[], &[], &fp_before);

    // Content edit to an EXISTING runtime source file (in place, so the
    // assertion rides on the content hash rather than on the path set).
    write_file(
        &dir.path().join("crates/perry-runtime/src/lib.rs"),
        b"pub fn rt_v2() {}\n",
    );

    // Only now plant the archives + the stamp recorded for the PRE-edit source,
    // so their mtimes are newer than every source and the mtime half of the gate
    // says "fresh". Anything but the stamp mismatch would let this pass.
    let runtime = dir
        .path()
        .join("target/perry-auto/release/libperry_runtime.a");
    let stdlib = dir
        .path()
        .join("target/perry-auto/release/libperry_stdlib.a");
    let stamp_path = dir.path().join("target/perry-auto/.perry-auto-build.stamp");
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_file(&runtime, b"!<arch>\n");
    write_file(&stdlib, b"!<arch>\n");
    write_file(&stamp_path, stamp_before.as_bytes());

    let fp_after = auto_optimized_source_fingerprint(dir.path(), &[]);
    let stamp_after = auto_optimized_build_stamp("key", None, &[], &[], &fp_after);
    assert_ne!(
        stamp_before, stamp_after,
        "a runtime source edit must rotate the build stamp"
    );
    assert!(
        !auto_optimized_archives_are_fresh(
            dir.path(),
            &runtime,
            &stdlib,
            &[],
            &stamp_path,
            &stamp_after,
        ),
        "archives stamped for the pre-edit runtime source must not pass the freshness gate"
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
fn undici_needs_shared_tokio() {
    // perry-ext-undici is network-I/O-family glue over the native fetch
    // stack; it rides the shared build (see the freshness.rs comment).
    assert!(binding_needs_shared_tokio("undici"));
}

/// The emitted-FFI → link derivation resolves to real well-known bindings.
/// The codegen prefix net routes `js_ioredis_*` / `js_undici_*` /
/// `js_node_forge_*` to these binding keys; each must exist in the shipped
/// `well_known_bindings.toml` and map to its `perry-ext-*` crate, or the
/// driver's routing loop would silently drop the flip.
#[test]
fn ext_prefix_binding_keys_resolve_to_wrapper_crates() {
    for (key, krate) in [
        ("ioredis", "perry-ext-ioredis"),
        ("undici", "perry-ext-undici"),
        ("node-forge", "perry-ext-node-forge"),
    ] {
        let binding = super::super::well_known::lookup_well_known(key)
            .unwrap_or_else(|| panic!("`{key}` must be a well-known binding"));
        assert_eq!(binding.krate, krate, "binding `{key}` routes to `{krate}`");
    }
}

/// The auto-build selection split: ioredis/undici carry their own tokio and
/// must ride the shared auto-optimize invocation, while node-forge is CPU-only
/// (routes async through perry-stdlib's spawn_blocking shim) and is auto-built
/// by the isolated leaf-build path in the driver's CPU-only branch.
#[test]
fn ext_binding_build_routing_split() {
    assert!(binding_needs_shared_tokio("ioredis"));
    assert!(binding_needs_shared_tokio("undici"));
    assert!(
        !binding_needs_shared_tokio("node-forge"),
        "node-forge is CPU-only and must not ride the shared-tokio invocation"
    );
}

#[test]
fn external_net_transport_keeps_tls_provider_without_bundled_net() {
    let mut features = std::collections::BTreeSet::from(["bundled-net", "tls"]);
    super::driver::finalize_tls_transport_features(&mut features, true, true);
    assert_eq!(
        features,
        std::collections::BTreeSet::from(["external-net-tls"]),
        "external net/http must keep TLS preflight without duplicate bundled-net symbols"
    );
}

#[test]
fn direct_tls_without_external_transport_keeps_legacy_umbrella() {
    let mut features = std::collections::BTreeSet::new();
    super::driver::finalize_tls_transport_features(&mut features, true, false);
    assert_eq!(features, std::collections::BTreeSet::from(["tls"]));
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
fn explicit_undici_import_routes_to_well_known_undici() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.native_module_imports.insert("undici".to_string());

    let modules = well_known_iteration_set(&ctx);

    assert!(modules.contains("undici"));
}

#[test]
fn node_test_usage_enables_mod_node_test_cross_feature() {
    // The `node:test` runner is the only retainer of the JSON serializer in an
    // otherwise plain program (`test::snapshot::assert_snapshot` calls
    // `js_json_stringify_full`), so it sits behind `mod-node-test`. Codegen
    // emits a direct `js_node_submod_install_test` call for any program that
    // imports it, so the gate MUST be on whenever that call is emitted or the
    // link dies with an undefined symbol.
    let dir = tempfile::tempdir().expect("tempdir");
    let empty_features: std::collections::BTreeSet<&'static str> =
        std::collections::BTreeSet::new();

    let mut with_test = CompilationContext::new(dir.path().to_path_buf());
    with_test.uses_node_test = true;
    let cross_on = auto_optimized_cross_features(&with_test, &empty_features, &[]);
    assert!(
        cross_on.iter().any(|f| f == "perry-runtime/mod-node-test"),
        "node:test usage should enable mod-node-test, got {cross_on:?}"
    );

    let without = CompilationContext::new(dir.path().to_path_buf());
    let cross_off = auto_optimized_cross_features(&without, &empty_features, &[]);
    assert!(
        !cross_off.iter().any(|f| f == "perry-runtime/mod-node-test"),
        "no node:test usage should leave mod-node-test off, got {cross_off:?}"
    );

    // `process.getBuiltinModule("node:test")` resolves at runtime, so the
    // runner has to be present even though no import statement names it.
    let mut dynamic = CompilationContext::new(dir.path().to_path_buf());
    dynamic.uses_get_builtin_module = true;
    let cross_dynamic = auto_optimized_cross_features(&dynamic, &empty_features, &[]);
    assert!(
        cross_dynamic
            .iter()
            .any(|f| f == "perry-runtime/mod-node-test"),
        "getBuiltinModule should enable mod-node-test, got {cross_dynamic:?}"
    );
}

#[test]
fn node_test_gate_keys_the_auto_optimize_cache() {
    // Two programs that differ ONLY in `node:test` usage build different
    // archives, so they must not share a `target/perry-auto-<hash>` dir.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut with_test = CompilationContext::new(dir.path().to_path_buf());
    with_test.uses_node_test = true;
    let without = CompilationContext::new(dir.path().to_path_buf());

    assert_ne!(
        auto_optimized_cache_key("", true, false, None, &with_test),
        auto_optimized_cache_key("", true, false, None, &without),
        "mod-node-test must participate in the auto-optimize cache key"
    );
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

    let mut dynamic = CompilationContext::new(dir.path().to_path_buf());
    dynamic.uses_get_builtin_module = true;
    let dynamic_features = auto_optimized_cross_features(&dynamic, &empty_features, &[]);
    assert!(
        dynamic_features
            .iter()
            .any(|f| f == "perry-runtime/mod-http2-constants"),
        "getBuiltinModule should enable mod-http2-constants, got {dynamic_features:?}"
    );
}

#[test]
fn http2_import_changes_optimized_libs_cache_key() {
    // #6468: the http2-constants usage bit participates in the auto-build cache
    // key, so a runtime built without the constant tables is never reused for a
    // program that imports `node:http2`.
    let dir = tempfile::tempdir().expect("tempdir");

    let base = CompilationContext::new(dir.path().to_path_buf());
    let key_without = auto_optimized_cache_key("", true, false, None, &base);

    let mut with_http2 = CompilationContext::new(dir.path().to_path_buf());
    with_http2.native_module_imports.insert("http2".to_string());
    let key_with = auto_optimized_cache_key("", true, false, None, &with_http2);

    assert_ne!(
        key_without, key_with,
        "an http2 import must change the auto-optimized cache key"
    );

    let mut dynamic = CompilationContext::new(dir.path().to_path_buf());
    dynamic.uses_get_builtin_module = true;
    assert_ne!(
        key_without,
        auto_optimized_cache_key("", true, false, None, &dynamic),
        "getBuiltinModule must change the auto-optimized cache key"
    );
}

#[test]
fn immediate_abort_requires_unwind_safe_reachability_and_changes_cache_identity() {
    let _guard = env_lock();
    let old_size_opt = std::env::var("PERRY_SIZE_OPT").ok();
    let old_size_panic = std::env::var("PERRY_SIZE_PANIC").ok();
    set_env_var("PERRY_SIZE_OPT", Some("z"));
    set_env_var("PERRY_SIZE_PANIC", Some("abort-immediate"));

    let ctx = CompilationContext::new(std::env::current_dir().expect("cwd"));
    let safe_mode = effective_size_panic_immediate_abort(true);
    let unsafe_mode = effective_size_panic_immediate_abort(false);
    let ordinary_key = auto_optimized_cache_key("", true, false, None, &ctx);
    let immediate_key = auto_optimized_cache_key("", true, safe_mode, None, &ctx);
    let unsafe_key = auto_optimized_cache_key("", false, unsafe_mode, None, &ctx);

    set_env_var("PERRY_SIZE_OPT", old_size_opt.as_deref());
    set_env_var("PERRY_SIZE_PANIC", old_size_panic.as_deref());

    assert!(safe_mode);
    assert!(!unsafe_mode);
    assert_ne!(ordinary_key, immediate_key);
    assert!(immediate_key.contains("+panicimm"));
    assert!(!unsafe_key.contains("+panicimm"));
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

/// Issue #76 follow-up: a bare `perry compile` of a `WebAssembly.*` program
/// must auto-build `perry-wasm-host` instead of hard-failing with
/// `libperry_wasm_host.a not found`. The build is a plain leaf
/// `cargo build --release -p perry-wasm-host` into `target/release`; drive it
/// with a fake cargo (so no real toolchain runs) and assert the resolved path.
#[cfg(unix)]
#[test]
fn wasm_host_auto_build_produces_staticlib_from_workspace_source() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = env_lock();
    let old_path = std::env::var_os("PATH");
    let old_cargo_target_dir = std::env::var_os("CARGO_TARGET_DIR");
    let old_workspace_root = std::env::var_os("PERRY_WORKSPACE_ROOT");

    let workspace = tempfile::tempdir().expect("tempdir");
    // A crate dir + workspace marker so `find_perry_workspace_root` (via
    // PERRY_WORKSPACE_ROOT) and the `crate_dir.is_dir()` guard both pass.
    for dir in [
        "crates/perry-wasm-host",
        "crates/perry-runtime",
        "crates/perry-ui-geisterhand",
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
  *"-p perry-wasm-host"*) ;;
  *) exit 43 ;;
esac
mkdir -p "$CARGO_TARGET_DIR/release"
printf '!<arch>\n' > "$CARGO_TARGET_DIR/release/libperry_wasm_host.a"
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
    std::env::set_var("PERRY_WORKSPACE_ROOT", workspace.path());

    let got = super::super::library_search::build_wasm_host_library(None, OutputFormat::Json, 0);

    set_env_var("PATH", old_path.as_deref().and_then(|v| v.to_str()));
    set_env_var(
        "CARGO_TARGET_DIR",
        old_cargo_target_dir.as_deref().and_then(|v| v.to_str()),
    );
    set_env_var(
        "PERRY_WORKSPACE_ROOT",
        old_workspace_root.as_deref().and_then(|v| v.to_str()),
    );

    assert_eq!(
        got.expect("missing wasm host lib should be built from workspace source"),
        target_dir.join("release/libperry_wasm_host.a")
    );
}

/// Binary/workspace skew: a cross-feature the on-disk checkout's
/// perry-runtime doesn't declare must be dropped (and reported), not passed
/// through to fail the entire cargo resolve — that failure's prebuilt
/// fallback links without the routed ext entrypoints and dies with
/// undefined `js_*` symbols far from the cause.
#[test]
fn retain_workspace_declared_features_drops_unknown_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_file(
        &dir.path().join("crates/perry-runtime/Cargo.toml"),
        b"[package]\nname = \"perry-runtime\"\n\n[features]\nfull = [\"dep:hidden-allocator\"]\nregex-engine = []\n\n[dependencies]\nmimalloc = { version = \"0.1\", optional = true }\nhidden-allocator = { version = \"0.1\", optional = true }\n",
    );
    write_file(
        &dir.path().join("crates/perry-stdlib/Cargo.toml"),
        b"[package]\nname = \"perry-stdlib\"\n\n[features]\ncrypto = []\n",
    );

    let mut cross_features = vec![
        "perry-runtime/full".to_string(),
        "perry-runtime/alloc-mimalloc".to_string(),
        "perry-runtime/mimalloc".to_string(),
        "perry-runtime/hidden-allocator".to_string(),
        "perry-stdlib/crypto".to_string(),
        "perry-stdlib/web-fetch".to_string(),
    ];
    let dropped = retain_workspace_declared_features(dir.path(), &mut cross_features);

    // `full` and `crypto` are declared features; `mimalloc` is an optional
    // dep with an implicit feature. `hidden-allocator` is referenced through
    // `dep:`, so Cargo does not expose an implicit same-named feature.
    assert_eq!(
        cross_features,
        vec![
            "perry-runtime/full".to_string(),
            "perry-runtime/mimalloc".to_string(),
            "perry-stdlib/crypto".to_string(),
        ]
    );
    assert_eq!(
        dropped,
        vec![
            "perry-runtime/alloc-mimalloc".to_string(),
            "perry-runtime/hidden-allocator".to_string(),
            "perry-stdlib/web-fetch".to_string(),
        ]
    );
}

/// A manifest without an explicit `[features]` table still declares implicit
/// features for optional dependencies, but must reject every other stale name.
#[test]
fn retain_workspace_declared_features_handles_missing_feature_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_file(
        &dir.path().join("crates/perry-runtime/Cargo.toml"),
        b"[package]\nname = \"perry-runtime\"\n\n[dependencies]\nmimalloc = { version = \"0.1\", optional = true }\n",
    );

    let mut cross_features = vec![
        "perry-runtime/mimalloc".to_string(),
        "perry-runtime/stale".to_string(),
    ];
    let dropped = retain_workspace_declared_features(dir.path(), &mut cross_features);

    assert_eq!(cross_features, vec!["perry-runtime/mimalloc".to_string()]);
    assert_eq!(dropped, vec!["perry-runtime/stale".to_string()]);
}

/// Fail-open: with no readable manifest (release tarball, partial checkout)
/// there is nothing trustworthy to filter against — every requested feature
/// must survive.
#[test]
fn retain_workspace_declared_features_keeps_all_without_manifests() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cross_features = vec![
        "perry-runtime/full".to_string(),
        "perry-runtime/alloc-mimalloc".to_string(),
    ];
    let dropped = retain_workspace_declared_features(dir.path(), &mut cross_features);
    assert!(dropped.is_empty());
    assert_eq!(cross_features.len(), 2);
}

/// Regression: the auto-optimize rebuild must always include
/// `perry-runtime/keepalive-anchors` in the cross-feature set. #6917 gated
/// the `#[used]` keepalive anchors behind the feature and only enabled it for
/// the bitcode-LTO path, under the assumption that the classic link path
/// "keeps every reachable runtime symbol via real undefined references from
/// the program's objects." That assumption is wrong: `#[no_mangle] pub extern
/// "C" fn` symbols that are only called from codegen (not from within the
/// perry-runtime crate) are dead-code-eliminated during staticlib archive
/// creation when no `#[used]` anchor pins them. The resulting
/// `libperry_runtime.a` is missing core symbols (`js_box_release`,
/// `js_bool_box_release`, `js_closure_set_box_capture_ptr`,
/// `js_link_path_module_parent`) and programs whose codegen emits calls to
/// them fail to link with "Undefined symbols for architecture arm64."
#[test]
fn auto_optimize_always_includes_keepalive_anchors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let empty_features: std::collections::BTreeSet<&'static str> =
        std::collections::BTreeSet::new();
    let ctx = CompilationContext::new(dir.path().to_path_buf());
    let cross = auto_optimized_cross_features(&ctx, &empty_features, &[]);
    assert!(
        cross.iter().any(|f| f == "perry-runtime/keepalive-anchors"),
        "auto_optimized_cross_features must always include \
         perry-runtime/keepalive-anchors so codegen-only #[no_mangle] symbols \
         survive into the staticlib archive, got {cross:?}"
    );
}

/// The `keepalive-anchors` feature must NOT be conditional on
/// `PERRY_LLVM_BITCODE_LINK` — the classic link path needs it too.
#[test]
fn auto_optimize_keepalive_anchors_not_bitcode_only() {
    let _guard = env_lock();
    let old_bitcode = std::env::var_os("PERRY_LLVM_BITCODE_LINK");
    std::env::remove_var("PERRY_LLVM_BITCODE_LINK");

    let dir = tempfile::tempdir().expect("tempdir");
    let empty_features: std::collections::BTreeSet<&'static str> =
        std::collections::BTreeSet::new();
    let ctx = CompilationContext::new(dir.path().to_path_buf());
    let cross = auto_optimized_cross_features(&ctx, &empty_features, &[]);

    set_env_var(
        "PERRY_LLVM_BITCODE_LINK",
        old_bitcode.as_deref().and_then(|v| v.to_str()),
    );

    assert!(
        cross.iter().any(|f| f == "perry-runtime/keepalive-anchors"),
        "keepalive-anchors must be present even without PERRY_LLVM_BITCODE_LINK, \
         got {cross:?}"
    );
}

/// The well-known flip drops `compression-brotli`/`compression-zstd` from the
/// stdlib rebuild on a stated premise: "The ext crate carries all codecs, so
/// nothing is lost by dropping them here."
///
/// #8005: that premise was a comment and nothing checked it. It is false for
/// the RAW one-shots — `js_zlib_deflate_raw_sync` and `js_zlib_inflate_raw_sync`
/// exist only in perry-stdlib — so the flip removed them from the link and
/// `test_gap_zlib_4917_level` failed with two undefined symbols, two stages
/// downstream of the decision that caused it.
///
/// This scans both crates for exported `js_zlib_*` symbols and requires the ext
/// surface to be a superset, minus an explicit shrink-only list. A name that
/// leaves stdlib, or gains an ext implementation, must be deleted from
/// `KNOWN_EXT_GAPS` in the same commit — an entry matching nothing FAILS, so
/// the list cannot rot into an alibi.
#[test]
fn ext_zlib_covers_every_stdlib_symbol_the_flip_strips() {
    /// Symbols perry-stdlib exports that perry-ext-zlib does not implement yet.
    /// SHRINKS ONLY. Every entry is reachable today only because the flip does
    /// not strip the feature that defines it; adding one is how #8005 happened.
    const KNOWN_EXT_GAPS: &[&str] = &[
        // Stream constructors — perry-ext-zlib owns streams through its own
        // dispatch (`js_ext_zlib_dispatch_method`) rather than these entry
        // points, so these are a naming difference, not a hole. Listed so the
        // superset check stays honest instead of being weakened to ignore them.
        "js_zlib_create_brotli_compress",
        "js_zlib_create_brotli_decompress",
        "js_zlib_create_deflate",
        "js_zlib_create_deflate_raw",
        "js_zlib_create_gunzip",
        "js_zlib_create_gzip",
        "js_zlib_create_inflate",
        "js_zlib_create_inflate_raw",
        "js_zlib_create_unzip",
        "js_zlib_create_zstd_compress",
        "js_zlib_create_zstd_decompress",
        // Pump/dispatch plumbing, supplied by the `external-zlib-pump` feature
        // the flip ADDS rather than strips.
        "js_zlib_has_active_handles",
        "js_zlib_native_dispatch",
        "js_zlib_process_pending",
    ];

    fn exported_zlib_symbols(dir: &Path) -> std::collections::BTreeSet<String> {
        let mut found = std::collections::BTreeSet::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&next) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for line in text.lines() {
                    if let Some(rest) = line.split("fn js_zlib_").nth(1) {
                        let name: String = rest
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                            .collect();
                        if !name.is_empty() {
                            found.insert(format!("js_zlib_{name}"));
                        }
                    }
                }
            }
        }
        found
    }

    let root = find_perry_workspace_root().expect("workspace root");
    let stdlib = exported_zlib_symbols(&root.join("crates/perry-stdlib/src"));
    let ext = exported_zlib_symbols(&root.join("crates/perry-ext-zlib/src"));

    // Live-subject check: a scan that found nothing would make every assertion
    // below vacuously true, which is precisely the failure mode this test is
    // about.
    assert!(
        stdlib.len() > 20 && ext.len() > 10,
        "symbol scan looks broken — stdlib {} / ext {}; the superset check \
         below would pass without proving anything",
        stdlib.len(),
        ext.len()
    );
    assert!(
        ext.contains("js_zlib_deflate_raw_sync") && ext.contains("js_zlib_inflate_raw_sync"),
        "#8005's pair must stay implemented in perry-ext-zlib; the flip strips \
         the stdlib feature that would otherwise supply them"
    );

    let missing: Vec<&String> = stdlib
        .iter()
        .filter(|name| !ext.contains(*name) && !KNOWN_EXT_GAPS.contains(&name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "perry-stdlib exports these `js_zlib_*` symbols and perry-ext-zlib does \
         not: {missing:?}. The well-known flip routes `node:zlib` to the ext \
         crate on the premise that it carries everything, so a symbol only \
         stdlib defines disappears from the link. Implement it in \
         perry-ext-zlib, or add it to KNOWN_EXT_GAPS with the reason."
    );

    let stale: Vec<&&str> = KNOWN_EXT_GAPS
        .iter()
        .filter(|name| !stdlib.contains(**name) || ext.contains(**name))
        .collect();
    assert!(
        stale.is_empty(),
        "these KNOWN_EXT_GAPS entries no longer describe reality — the symbol \
         left perry-stdlib or gained an ext implementation: {stale:?}. Delete \
         them; a list that outlives its entries stops being a ratchet."
    );
}

/// Regression: a program that references `WebAssembly.*` must get the
/// `perry-runtime/wasm-host` cross feature so the runtime archive carries
/// `js_webassembly_*` symbol definitions. Without it the link fails with
/// `_js_webassembly_module_new` undefined (issue #76).
#[test]
fn wasm_usage_enables_wasm_host_cross_feature() {
    let workspace_root = find_perry_workspace_root().expect("workspace root");
    let mut ctx = CompilationContext::new(workspace_root);
    ctx.needs_wasm_runtime = true;

    let features = compute_required_features(
        &ctx.native_module_imports,
        ctx.uses_fetch,
        ctx.uses_crypto_builtins,
    );
    let cross = auto_optimized_cross_features(&ctx, &features, &[]);
    assert!(
        cross.iter().any(|f| f == "perry-runtime/wasm-host"),
        "needs_wasm_runtime=true must add perry-runtime/wasm-host to the \
         cross-feature set so the runtime archive defines js_webassembly_* \
         symbols; got: {cross:?}"
    );
}

/// Regression: a program that does NOT reference `WebAssembly.*` must NOT
/// get the `perry-runtime/wasm-host` cross feature — non-wasm programs
/// don't pay for wasmi (the feature is deliberately kept out of `default`).
#[test]
fn non_wasm_usage_does_not_enable_wasm_host_cross_feature() {
    let workspace_root = find_perry_workspace_root().expect("workspace root");
    let ctx = CompilationContext::new(workspace_root);

    let features = compute_required_features(
        &ctx.native_module_imports,
        ctx.uses_fetch,
        ctx.uses_crypto_builtins,
    );
    let cross = auto_optimized_cross_features(&ctx, &features, &[]);
    assert!(
        !cross.iter().any(|f| f == "perry-runtime/wasm-host"),
        "needs_wasm_runtime=false must NOT add perry-runtime/wasm-host — \
         non-wasm programs must not link the wasmi host; got: {cross:?}"
    );
}

/// Regression: the auto-optimize cache key must differ between a wasm-using
/// program and a non-wasm program so cargo doesn't serve a cached non-wasm
/// runtime archive (missing `js_webassembly_*`) to a wasm program, or vice
/// versa (an archive carrying unresolved `perry_wasm_host_*` refs).
#[test]
fn wasm_usage_changes_auto_optimize_cache_key() {
    let workspace_root = find_perry_workspace_root().expect("workspace root");
    let ctx_no_wasm = CompilationContext::new(workspace_root.clone());
    let mut ctx_wasm = CompilationContext::new(workspace_root);
    ctx_wasm.needs_wasm_runtime = true;

    let features = compute_required_features(
        &ctx_no_wasm.native_module_imports,
        ctx_no_wasm.uses_fetch,
        ctx_no_wasm.uses_crypto_builtins,
    );
    let feature_arg = features_to_cargo_arg(&features);
    let key_no_wasm = auto_optimized_cache_key(&feature_arg, true, false, None, &ctx_no_wasm);
    let key_wasm = auto_optimized_cache_key(&feature_arg, true, false, None, &ctx_wasm);
    assert_ne!(
        key_no_wasm, key_wasm,
        "wasm usage must change the cache key so the target dirs don't collide"
    );
}
