use super::no_auto::{build_missing_prebuilt_ext_lib, resolve_prebuilt_ext_libs};
use super::*;
use std::path::Path;

use crate::commands::stdlib_features::compute_required_features;
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

#[test]
fn missing_ext_archive_names_emitted_symbols_and_their_providers() {
    let emitted = BTreeSet::from([
        "js_bun_tcp_nm_install".to_string(),
        "js_ext_http_nm_install".to_string(),
    ]);
    let definitions = HashSet::new();
    let missing = missing_ext_archive_diagnostics(&emitted, &definitions, &[], None);
    assert_eq!(missing.len(), 2);
    assert!(missing.iter().any(|line| {
        line.contains("js_bun_tcp_nm_install")
            && line.contains("libperry_ext_net.a")
            && line.contains("cargo build --release -p perry-ext-net")
    }));
    assert!(missing.iter().any(|line| {
        line.contains("js_ext_http_nm_install")
            && line.contains("libperry_ext_http.a")
            && line.contains("cargo build --release -p perry-ext-http")
    }));

    let resolved = [PathBuf::from("/archive/libperry_ext_net.a")];
    let definitions = HashSet::from(["js_ext_http_nm_install".to_string()]);
    assert!(missing_ext_archive_diagnostics(&emitted, &definitions, &resolved, None).is_empty());
}

#[test]
fn web_streams_remain_bundled_when_well_known_wrappers_are_enabled() {
    let _guard = env_lock();
    let old_disable_well_known = std::env::var("PERRY_DISABLE_WELL_KNOWN").ok();
    set_env_var("PERRY_DISABLE_WELL_KNOWN", None);
    let routing = native_routing();
    set_env_var(
        "PERRY_DISABLE_WELL_KNOWN",
        old_disable_well_known.as_deref(),
    );

    assert_eq!(routing.wrapper("streams"), None);
    assert!(!routing.unprovided("streams"));
    assert!(
        crate::commands::stdlib_features::module_to_features("streams")
            .contains(&"bundled-streams")
    );
}

/// The emitted-FFI → link derivation resolves to real well-known bindings.
/// The codegen prefix net routes `js_undici_*` to these
/// binding keys; each must exist in the shipped `well_known_bindings.toml`
/// and map to its `perry-ext-*` crate, or the driver's routing loop would
/// silently drop the flip.
#[test]
fn ext_prefix_binding_keys_resolve_to_wrapper_crates() {
    for (key, krate) in [("undici", "perry-ext-undici")] {
        let binding = super::super::well_known::lookup_well_known(key)
            .unwrap_or_else(|| panic!("`{key}` must be a well-known binding"));
        assert_eq!(binding.krate, krate, "binding `{key}` routes to `{krate}`");
    }
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
    // #10466 — deliberately NOT "http"/"https" here: importing either now
    // triggers `build_http_client_pump_stdlib`'s on-demand rebuild (a real
    // cargo invocation), which this test's fake `PERRY_LIB_DIR` archives
    // (raw `!<arch>\n` placeholders, not real cargo output) can't stand in
    // for, and which would turn this fast unit test into a slow, real build.
    // That new behavior has its own coverage below
    // (`no_auto_http_client_import_rebuilds_pump_stdlib_with_ext_http`).
    // This test's job is unrelated: confirm `resolve_prebuilt_ext_libs` still
    // finds multiple well-known archives via `PERRY_LIB_DIR` when no rebuild
    // trigger is present.
    let net = super::super::well_known::lookup_well_known("net").expect("net well-known binding");
    let ws = super::super::well_known::lookup_well_known("ws").expect("ws well-known binding");
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
    std::fs::write(&net_lib, b"!<arch>\n").expect("write fake net archive");
    std::fs::write(&ws_lib, b"!<arch>\n").expect("write fake ws archive");

    set_env_var(
        "PERRY_LIB_DIR",
        Some(dir.path().to_str().expect("utf8 temp path")),
    );
    set_env_var("PERRY_RUNTIME_DIR", None);
    set_env_var("PERRY_DISABLE_WELL_KNOWN", None);

    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
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

/// tokio lane L4: a `tls` import routes `net` — the TLS client and every
/// client TLSSocket method are perry-ext-net's, and bundled `net` is gone.
#[test]
fn tls_import_routes_net_wrapper() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.native_module_imports.insert("node:tls".to_string());
    // `well_known_iteration_set` reads PERRY_FORCE_WELL_KNOWN, which other
    // tests in this binary mutate; hold the env lock across the read.
    let _guard = env_lock();
    assert!(well_known_iteration_set(&ctx).contains("net"));
}

/// tokio lane L4: perry-stdlib's bundled `net` / `ws` copies are deleted, so
/// PERRY_DISABLE_WELL_KNOWN=1 must still put those two wrappers on the link
/// line (there is no copy to revert to) while every other binding keeps
/// reverting to perry-stdlib (`events` has no wrapper at all: #10508).
#[test]
fn disabled_flip_still_routes_sole_provider_wrappers() {
    let _guard = env_lock();
    let saved: Vec<_> = [
        "PERRY_LIB_DIR",
        "PERRY_RUNTIME_DIR",
        "PERRY_DISABLE_WELL_KNOWN",
    ]
    .iter()
    .map(|k| (*k, std::env::var(k).ok()))
    .collect();

    let dir = tempfile::tempdir().expect("tempdir");
    let mut archives = Vec::new();
    for module in ["net", "ws"] {
        let binding = super::super::well_known::lookup_well_known(module).expect("binding");
        let lib = dir
            .path()
            .join(super::super::well_known::ext_staticlib_filename(
                &binding.lib,
                rust_target_triple(None),
            ));
        std::fs::write(&lib, b"!<arch>\n").expect("write fake archive");
        archives.push(lib);
    }
    set_env_var("PERRY_LIB_DIR", dir.path().to_str());
    set_env_var("PERRY_RUNTIME_DIR", None);
    set_env_var("PERRY_DISABLE_WELL_KNOWN", Some("1"));

    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    for module in ["net", "ws", "events"] {
        ctx.native_module_imports.insert(module.to_string());
    }
    let libs = resolve_no_auto_optimized_libs(&ctx, None, OutputFormat::Json, 0);
    let routed = super::routed_modules(&ctx);

    for (key, value) in &saved {
        set_env_var(key, value.as_deref());
    }

    assert!(
        libs.well_known_libs.contains(&archives[0]),
        "net: {libs:?}",
        libs = libs.well_known_libs
    );
    assert!(
        libs.well_known_libs.contains(&archives[1]),
        "ws: {libs:?}",
        libs = libs.well_known_libs
    );
    assert_eq!(
        routed,
        std::collections::BTreeSet::from(["net".to_string(), "ws".to_string()])
    );
    assert!(super::wrapper_is_sole_provider("node:net"));
    assert!(!super::wrapper_is_sole_provider("tls"));
}

/// #10466 — the flip side of the test above: when the program DOES import
/// `http`, no-auto now rebuilds `perry-stdlib-static` (with
/// `external-http-client-pump`) and `perry-ext-http` together, and the
/// rebuilt `perry-ext-http` archive takes the place of whatever
/// `resolve_prebuilt_ext_libs` would otherwise have found on disk for it.
/// This does a real (if small) cargo build, so it's slower than the rest of
/// this file — that's the trade-off for exercising the actual rebuild path
/// rather than re-asserting the pass-through plumbing against a mock.
#[test]
fn no_auto_http_client_import_rebuilds_pump_stdlib_with_ext_http() {
    let _guard = env_lock();
    let mut ctx = CompilationContext::new(
        find_perry_workspace_root().expect("workspace root for this checkout"),
    );
    ctx.native_module_imports.insert("http".to_string());
    let libs = resolve_no_auto_optimized_libs(&ctx, None, OutputFormat::Json, 0);

    let stdlib = libs
        .stdlib
        .as_ref()
        .expect("http import should trigger the http-client-pump stdlib rebuild");
    assert!(
        stdlib.ends_with("libperry_stdlib.a") || stdlib.ends_with("perry_stdlib.lib"),
        "unexpected stdlib archive name: {stdlib:?}"
    );
    let ext_http_in_well_known = libs.well_known_libs.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains("perry_ext_http"))
    });
    assert!(
        ext_http_in_well_known,
        "expected the freshly-rebuilt perry-ext-http archive in well_known_libs, got {:?}",
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

#[test]
fn bun_usage_enables_cli_utility_runtime_pack() {
    let dir = tempfile::tempdir().expect("tempdir");
    let empty_features: std::collections::BTreeSet<&'static str> =
        std::collections::BTreeSet::new();
    let mut imported = CompilationContext::new(dir.path().to_path_buf());
    imported.native_module_imports.insert("bun".to_string());
    let imported_features = auto_optimized_cross_features(&imported, &empty_features, &[]);
    assert!(
        imported_features
            .iter()
            .any(|feature| feature == "perry-runtime/bun-cli-utils"),
        "a bun import must retain #9600's runtime backends: {imported_features:?}"
    );

    let mut platform = CompilationContext::new(dir.path().to_path_buf());
    platform.bun_platform = true;
    let platform_features = auto_optimized_cross_features(&platform, &empty_features, &[]);
    assert!(
        platform_features
            .iter()
            .any(|feature| feature == "perry-runtime/bun-cli-utils"),
        "--platform bun must retain #9600's runtime backends: {platform_features:?}"
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

/// Both default and optimized builds use the same binding provider. Keep a
/// concrete ABI inventory: removing the bundled provider must not make its
/// former superset witness vacuous.
#[test]
fn ext_zlib_is_the_only_provider_and_covers_the_linked_surface() {
    let root = find_perry_workspace_root().expect("workspace root");
    assert!(!root.join("crates/perry-stdlib/src/zlib.rs").exists());
    let source = std::fs::read_to_string(root.join("crates/perry-ext-zlib/src/lib.rs")).unwrap()
        + &std::fs::read_to_string(root.join("crates/perry-ext-zlib/src/stream.rs")).unwrap();
    for family in [
        "gzip",
        "gunzip",
        "deflate",
        "inflate",
        "deflate_raw",
        "inflate_raw",
        "unzip",
        "brotli_compress",
        "brotli_decompress",
        "zstd_compress",
        "zstd_decompress",
    ] {
        for symbol in [
            format!("js_zlib_{family}_sync"),
            format!("js_zlib_{family}"),
            format!("js_zlib_create_{family}"),
            format!("js_zlib_{family}_init"),
        ] {
            assert!(source.contains(&symbol), "missing binding ABI: {symbol}");
        }
    }
    for retired in [
        "scan_zlib_roots",
        "js_zlib_process_pending",
        "js_zlib_has_active_handles",
    ] {
        assert!(
            !source.contains(retired),
            "retired zlib plumbing: {retired}"
        );
    }
    let manifest = std::fs::read_to_string(root.join("crates/perry-stdlib/Cargo.toml")).unwrap();
    assert!(manifest.contains("compression-gzip = [\"dep:perry-ext-zlib\"]"));
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

#[test]
fn gc_instrument_knobs_match_the_runtime() {
    // The compiler decides whether to link the instruments; the runtime aborts
    // when a knob it cannot serve is set. Both must name the same knobs.
    let root = super::super::find_perry_workspace_root().unwrap();
    let src =
        std::fs::read_to_string(root.join("crates/perry-runtime/src/gc/instruments.rs")).unwrap();
    let start = src.find("INSTRUMENT_KNOBS: &[&str] = &[").unwrap();
    let end = start + src[start..].find("];").unwrap();
    let runtime: Vec<&str> = src[start..end].split('"').skip(1).step_by(2).collect();
    assert_eq!(runtime, super::features::GC_INSTRUMENT_KNOBS);
}

#[test]
fn hot_diag_knobs_match_the_runtime() {
    // #10572: same pairing as the GC instruments, for `hot_diag`'s probes.
    let root = super::super::find_perry_workspace_root().unwrap();
    let src = std::fs::read_to_string(root.join("crates/perry-runtime/src/hot_diag.rs")).unwrap();
    let start = src.find("HOT_DIAG_KNOBS: &[&str] = &[").unwrap();
    let end = start + src[start..].find("];").unwrap();
    let runtime: Vec<&str> = src[start..end].split('"').skip(1).step_by(2).collect();
    assert_eq!(runtime, super::features::HOT_DIAG_KNOBS);
}

mod no_auto_http_graph;

/// Decision 69: codegen and the linker read ONE routing decision. For each
/// representative import, in both modes and with the flip on and off, the
/// install symbol codegen emits for the module's namespace, the entry
/// prologue's wrapper hooks and the wrapper calls codegen may emit must name
/// exactly the wrapper archives the linker links: auto mode links the
/// archives of `routed_modules` (driver.rs), no-auto mode what
/// `resolve_prebuilt_ext_libs` finds for them.
#[test]
fn codegen_install_symbols_match_the_linked_wrappers() {
    let _guard = env_lock();
    let saved: Vec<_> = [
        "PERRY_LIB_DIR",
        "PERRY_RUNTIME_DIR",
        "PERRY_DISABLE_WELL_KNOWN",
        "PERRY_NO_AUTO_OPTIMIZE",
        "PERRY_FORCE_WELL_KNOWN",
    ]
    .iter()
    .map(|k| (*k, std::env::var(k).ok()))
    .collect();

    let modules = ["http", "node:https", "net", "zlib", "ws"];
    let dir = tempfile::tempdir().expect("tempdir");
    for module in modules {
        let binding = super::super::well_known::lookup_well_known(module).expect("binding");
        let lib = dir
            .path()
            .join(super::super::well_known::ext_staticlib_filename(
                &binding.lib,
                rust_target_triple(None),
            ));
        std::fs::write(&lib, b"!<arch>\n").expect("write fake archive");
    }
    set_env_var("PERRY_LIB_DIR", dir.path().to_str());
    set_env_var("PERRY_RUNTIME_DIR", None);
    set_env_var("PERRY_FORCE_WELL_KNOWN", None);

    // Wrapper symbols codegen emits for each module beyond its install.
    let wrapper_calls = [
        ("http", "js_node_http_create_server_with_options"),
        ("net", "js_ext_net_nm_install"),
    ];
    let mut rows = Vec::new();
    for no_auto in [false, true] {
        for flip in [true, false] {
            set_env_var("PERRY_NO_AUTO_OPTIMIZE", no_auto.then_some("1"));
            set_env_var("PERRY_DISABLE_WELL_KNOWN", (!flip).then_some("1"));
            let mut ctx = CompilationContext::new(dir.path().to_path_buf());
            for module in modules {
                ctx.native_module_imports.insert(module.to_string());
            }
            let routing = &ctx.native_routing;
            let routed = routed_modules(&ctx);
            // The linker's wrapper archives, by lib stem.
            let linked: std::collections::BTreeSet<String> = if no_auto {
                resolve_prebuilt_ext_libs(&routed, None, OutputFormat::Json, 0)
                    .iter()
                    .map(|p| {
                        let name = p.file_name().unwrap().to_str().unwrap();
                        name.trim_start_matches("lib")
                            .trim_end_matches(".a")
                            .to_string()
                    })
                    .collect()
            } else {
                routed
                    .iter()
                    .map(|m| {
                        super::super::well_known::lookup_well_known(m)
                            .unwrap()
                            .lib
                            .clone()
                    })
                    .collect()
            };
            let lib_of = |m: &str| {
                super::super::well_known::lookup_well_known(m)
                    .unwrap()
                    .lib
                    .clone()
            };
            for module in modules {
                let install = routing.install_symbol(module);
                let hook = perry_codegen::ext_registry::wrapper_install_hook(&lib_of(module));
                if linked.contains(&lib_of(module)) && hook.is_some() {
                    assert_eq!(
                        install, hook,
                        "{module} (no_auto={no_auto}, flip={flip}): its wrapper is linked, \
                         so its namespace must install the wrapper's hook"
                    );
                } else {
                    assert!(
                        install.is_none_or(|s| !s.starts_with("js_ext_")),
                        "{module} (no_auto={no_auto}, flip={flip}): wrapper hook {install:?} \
                         emitted, but the linker links {linked:?}"
                    );
                }
                rows.push(format!("{no_auto} {flip} {module} {install:?}"));
            }
            for (module, symbol) in wrapper_calls {
                assert_eq!(
                    routing.serves(symbol),
                    linked.contains(&lib_of(module)),
                    "{symbol} (no_auto={no_auto}, flip={flip}): linked {linked:?}"
                );
            }
            let hooks = routing
                .wrapper_install_hooks(well_known_iteration_set(&ctx).iter().map(String::as_str));
            let expected_hooks: Vec<String> = linked
                .iter()
                .filter_map(|lib| perry_codegen::ext_registry::wrapper_install_hook(lib))
                .map(str::to_string)
                .collect();
            assert_eq!(hooks, expected_hooks, "no_auto={no_auto}, flip={flip}");
        }
    }

    for (key, value) in &saved {
        set_env_var(key, value.as_deref());
    }

    // The concrete answer, so a change to the decision itself is visible.
    let expect = |no_auto: bool, flip: bool, module: &str| {
        rows.iter()
            .find(|r| r.starts_with(&format!("{no_auto} {flip} {module} ")))
            .unwrap()
            .clone()
    };
    for no_auto in [false, true] {
        assert!(expect(no_auto, true, "http").ends_with("Some(\"js_ext_http_nm_install\")"));
        assert!(expect(no_auto, true, "node:https").ends_with("Some(\"js_ext_http_nm_install\")"));
        assert!(expect(no_auto, false, "http").ends_with("Some(\"js_nm_install_http\")"));
        assert!(expect(no_auto, false, "node:https").ends_with("Some(\"js_nm_install_http\")"));
        for flip in [true, false] {
            assert!(expect(no_auto, flip, "zlib").ends_with("Some(\"js_ext_zlib_nm_install\")"));
            assert!(expect(no_auto, flip, "net").ends_with("Some(\"js_ext_net_nm_install\")"));
            assert!(expect(no_auto, flip, "ws").ends_with("None"));
        }
    }
}

#[test]
fn missing_http_archive_explains_installed_and_source_options() {
    let emitted = BTreeSet::from(["js_ext_http_nm_install".to_string()]);
    let diagnostics = missing_ext_archive_diagnostics(&emitted, &HashSet::new(), &[], None);
    assert_eq!(diagnostics.len(), 1);
    let message = &diagnostics[0];
    assert!(message.contains("installed Perry"), "{message}");
    assert!(message.contains("matching extension archive"), "{message}");
    assert!(
        message.contains("cargo build --release -p perry-ext-http"),
        "{message}"
    );
}

#[test]
fn dynamic_import_options_install_bun_cli_utils() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut with = CompilationContext::new(dir.path().to_path_buf());
    with.uses_dynamic_import_options = true;
    let cross = auto_optimized_cross_features(&with, &std::collections::BTreeSet::new(), &[]);
    assert!(cross
        .iter()
        .any(|feature| feature == "perry-runtime/bun-cli-utils"));
}

#[test]
fn data_url_dynamic_import_installs_dyn_eval() {
    let dir = tempfile::tempdir().expect("tempdir");
    let empty_features = std::collections::BTreeSet::new();
    let mut with_data_url = CompilationContext::new(dir.path().to_path_buf());
    with_data_url.uses_data_url_dynamic_import = true;
    let cross = auto_optimized_cross_features(&with_data_url, &empty_features, &[]);
    assert!(
        cross.iter().any(|f| f == "perry-runtime/dyn-eval"),
        "data URL modules require the dyn-eval runtime, got {cross:?}"
    );
}
