use super::wrapper_libs::resolve_prebuilt_ext_libs;
use super::*;

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
fn prebuilt_well_known_archives_resolve_through_perry_lib_dir() {
    let _guard = env_lock();
    let old_lib_dir = std::env::var("PERRY_LIB_DIR").ok();
    let old_runtime_dir = std::env::var("PERRY_RUNTIME_DIR").ok();
    let old_disable_well_known = std::env::var("PERRY_DISABLE_WELL_KNOWN").ok();

    let dir = tempfile::tempdir().expect("tempdir");
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
    let libs = resolve_prebuilt_ext_libs(&routed_modules(&ctx), None, OutputFormat::Json);

    set_env_var("PERRY_LIB_DIR", old_lib_dir.as_deref());
    set_env_var("PERRY_RUNTIME_DIR", old_runtime_dir.as_deref());
    set_env_var(
        "PERRY_DISABLE_WELL_KNOWN",
        old_disable_well_known.as_deref(),
    );

    assert!(
        libs.contains(&net_lib),
        "expected the well-known libs to include {net_lib:?}, got {libs:?}"
    );
    assert!(
        libs.contains(&ws_lib),
        "expected the well-known libs to include {ws_lib:?}, got {libs:?}"
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
    let routed = super::routed_modules(&ctx);
    let libs = resolve_prebuilt_ext_libs(&routed, None, OutputFormat::Json);

    for (key, value) in &saved {
        set_env_var(key, value.as_deref());
    }

    assert!(libs.contains(&archives[0]), "net: {libs:?}");
    assert!(libs.contains(&archives[1]), "ws: {libs:?}");
    assert_eq!(
        routed,
        std::collections::BTreeSet::from(["net".to_string(), "ws".to_string()])
    );
    assert!(super::wrapper_is_sole_provider("node:net"));
    assert!(!super::wrapper_is_sole_provider("tls"));
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

/// The prebuilt stdlib carries the `external-*` adapters (#10466): an `http`
/// import installs the ones a routed perry-ext-http needs, links the prebuilt
/// wrapper, and builds nothing.
#[test]
fn http_import_installs_the_external_adapters_of_the_prebuilt_stdlib() {
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
    set_env_var("PERRY_LIB_DIR", dir.path().to_str());
    set_env_var("PERRY_RUNTIME_DIR", None);
    set_env_var("PERRY_DISABLE_WELL_KNOWN", None);
    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.native_module_imports.insert("http".to_string());
    for module in routed_modules(&ctx) {
        let binding = super::super::well_known::lookup_well_known(&module).expect("binding");
        let lib = dir
            .path()
            .join(super::super::well_known::ext_staticlib_filename(
                &binding.lib,
                rust_target_triple(None),
            ));
        std::fs::write(&lib, b"!<arch>\n").expect("write fake archive");
    }
    let libs = build_optimized_libs(&ctx, None, &[], OutputFormat::Json, 0);
    for (key, value) in &saved {
        set_env_var(key, value.as_deref());
    }

    let crate::commands::stdlib_installs::FeatureInstalls::Selected(installs) =
        &libs.stdlib_installs
    else {
        panic!("a prebuilt link selects its stdlib installs");
    };
    for adapter in [
        "external-http-client-pump",
        "external-http-server-pump",
        "external-net-tls",
    ] {
        assert!(installs.contains(adapter), "{adapter}: {installs:?}");
    }
    assert!(
        libs.well_known_libs.iter().any(|p| p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains("perry_ext_http"))),
        "{:?}",
        libs.well_known_libs
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
                resolve_prebuilt_ext_libs(&routed, None, OutputFormat::Json)
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

/// perry never builds an archive while compiling a program, so the workspace
/// build must produce every archive a program can link: the runtime, the
/// stdlib, the WebAssembly host and every routed wrapper of
/// `well_known_bindings.toml`, in ONE Cargo graph. `default-members` (plain
/// `cargo build`) and the `build-perry` alias (a build that adds packages)
/// are that set.
#[test]
fn default_members_match_the_build_perry_alias() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(manifest) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return;
    };
    let Ok(config) = std::fs::read_to_string(root.join(".cargo/config.toml")) else {
        return;
    };
    let manifest: toml::Table = manifest.parse().expect("workspace manifest");
    let config: toml::Table = config.parse().expect("cargo config");
    let members: BTreeSet<String> = manifest["workspace"]["default-members"]
        .as_array()
        .expect("default-members")
        .iter()
        .map(|m| {
            m.as_str()
                .unwrap()
                .trim_start_matches("crates/")
                .to_string()
        })
        .filter(|m| m != "perry-container-compose")
        .collect();
    let alias = config["alias"]["build-perry"].as_array().expect("alias");
    let alias: BTreeSet<String> = alias
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|w| w[0] == "-p")
        .map(|w| w[1].clone())
        .collect();
    assert_eq!(
        members, alias,
        "default-members and `cargo build-perry` differ"
    );
    // The instrumented variant builds the same archives, minus the compiler and
    // the WebAssembly host (neither bundles the runtime).
    let instr: BTreeSet<String> = config["alias"]["build-perry-instr"]
        .as_array()
        .expect("instr alias")
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|w| w[0] == "-p")
        .map(|w| w[1].clone())
        .collect();
    let mut archives = alias.clone();
    archives.remove("perry");
    archives.remove("perry-wasm-host");
    assert_eq!(
        archives, instr,
        "`cargo build-perry-instr` must build the archives of `cargo build-perry`"
    );
    for required in [
        "perry",
        "perry-runtime-static",
        "perry-stdlib-static",
        "perry-wasm-host",
    ] {
        assert!(
            members.contains(required),
            "{required} missing from default-members"
        );
    }
    let bindings = std::fs::read_to_string(root.join("crates/perry/well_known_bindings.toml"))
        .expect("well_known_bindings.toml");
    let bindings: toml::Table = bindings.parse().expect("bindings");
    let bindings = bindings["bindings"].as_table().expect("[bindings]");
    for binding in bindings.values() {
        if let Some(krate) = binding.get("crate").and_then(|c| c.as_str()) {
            assert!(
                members.contains(krate),
                "routed wrapper {krate} is not built with perry (default-members)"
            );
        }
    }
}

/// The instrumented variant's archives sit beside the default ones as
/// `<name>_instr.<ext>`, runtime, stdlib and wrappers alike, on every OS; the
/// default variant keeps the plain names.
#[test]
fn the_instrumented_variant_names_its_archives_with_an_instr_suffix() {
    use super::features::variant_archive_name_for as name;
    for (default, instr) in [
        ("libperry_runtime.a", "libperry_runtime_instr.a"),
        ("libperry_stdlib.a", "libperry_stdlib_instr.a"),
        ("libperry_ext_http.a", "libperry_ext_http_instr.a"),
        ("perry_runtime.lib", "perry_runtime_instr.lib"),
        ("perry_ext_ws.lib", "perry_ext_ws_instr.lib"),
    ] {
        assert_eq!(name(default, false), default);
        assert_eq!(name(default, true), instr);
    }
    // A name that is not an archive is left alone.
    assert_eq!(name("perry", true), "perry");
}

#[test]
fn deserializers_install_regex_without_a_regex_literal() {
    // Other compile fixtures can leave process-global dynamic-code sites.
    // A fresh child isolates this case without draining another test's state.
    const CHILD: &str = "PERRY_TEST_DESERIALIZER_FEATURES_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::compile::optimized_libs::tests::deserializers_install_regex_without_a_regex_literal",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let features = BTreeSet::new();
    let plain = CompilationContext::new(dir.path().to_path_buf());
    assert!(!auto_optimized_cross_features(&plain, &features, &[])
        .iter()
        .any(|f| f == "perry-runtime/regex-engine"));
    for module in ["v8", "worker_threads"] {
        let mut ctx = CompilationContext::new(dir.path().to_path_buf());
        ctx.native_module_imports.insert(module.to_string());
        assert!(auto_optimized_cross_features(&ctx, &features, &[])
            .iter()
            .any(|f| f == "perry-runtime/regex-engine"));
    }
    let mut ipc = CompilationContext::new(dir.path().to_path_buf());
    ipc.uses_proc_ipc = true;
    assert!(auto_optimized_cross_features(&ipc, &features, &[])
        .iter()
        .any(|f| f == "perry-runtime/regex-engine"));
}
