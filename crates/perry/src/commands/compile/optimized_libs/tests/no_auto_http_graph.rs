//! No-auto HTTP rebuild tests: the miniature-workspace cargo-graph checks
//! (#11174 runtime default features, one shared graph for every linked
//! archive). Split out of `optimized_libs/tests.rs` to stay under the
//! 2,000-line cap (#10750).

use super::*;

// #11174: a no-auto HTTP rebuild bundles runtime code into the stdlib archive.
// Exercise Cargo feature unification with a real miniature workspace; the
// runtime refuses to build if the command silently drops its default engines.
#[test]
fn no_auto_http_pump_preserves_runtime_defaults() {
    let _guard = env_lock();
    let workspace = tempfile::tempdir().expect("workspace");
    std::fs::create_dir_all(workspace.path().join("crates/perry-ui-geisterhand"))
        .expect("workspace marker");
    write_file(
        &workspace.path().join("Cargo.toml"),
        br#"
[workspace]
resolver = "2"
members = ["crates/perry-runtime", "crates/perry-runtime-static", "crates/perry-stdlib-static", "crates/perry-ext-http"]
"#,
    );
    write_runtime_static_crate(workspace.path());
    write_file(
        &workspace.path().join("crates/perry-runtime/Cargo.toml"),
        br#"
[package]
name = "perry-runtime"
version = "0.1.0"
edition = "2021"
[features]
default = ["regex-engine", "temporal", "url-engine"]
regex-engine = []
temporal = []
url-engine = []
stdlib = []
"#,
    );
    write_file(
        &workspace.path().join("crates/perry-runtime/src/lib.rs"),
        br#"
#[cfg(not(all(feature = "regex-engine", feature = "temporal", feature = "url-engine")))]
compile_error!("no-auto HTTP build lost runtime defaults");
pub fn runtime_probe() -> u32 { 42 }
"#,
    );
    write_file(
        &workspace
            .path()
            .join("crates/perry-stdlib-static/Cargo.toml"),
        br#"
[package]
name = "perry-stdlib-static"
version = "0.1.0"
edition = "2021"
[lib]
name = "perry_stdlib"
crate-type = ["staticlib"]
[dependencies]
perry-runtime = { path = "../perry-runtime", default-features = false, features = ["stdlib"] }
perry-stdlib = { path = "../perry-stdlib" }
"#,
    );
    write_file(
        &workspace
            .path()
            .join("crates/perry-stdlib-static/src/lib.rs"),
        br#"
#[no_mangle]
pub extern "C" fn stdlib_probe() -> u32 { perry_runtime::runtime_probe() }
"#,
    );
    write_file(
        &workspace.path().join("crates/perry-stdlib/Cargo.toml"),
        br#"
[package]
name = "perry-stdlib"
version = "0.1.0"
edition = "2021"
[features]
external-http-client-pump = []
"#,
    );
    write_file(
        &workspace.path().join("crates/perry-stdlib/src/lib.rs"),
        b"",
    );
    write_file(
        &workspace.path().join("crates/perry-ext-http/Cargo.toml"),
        br#"
[package]
name = "perry-ext-http"
version = "0.1.0"
edition = "2021"
[lib]
name = "perry_ext_http"
crate-type = ["staticlib"]
"#,
    );
    write_file(
        &workspace.path().join("crates/perry-ext-http/src/lib.rs"),
        b"",
    );
    let old_root = std::env::var_os("PERRY_WORKSPACE_ROOT");
    std::env::set_var("PERRY_WORKSPACE_ROOT", workspace.path());
    let built = super::super::no_auto::build_coherent_stdlib(
        workspace.path().to_path_buf(),
        &[(
            "perry-ext-http".to_string(),
            crate::commands::compile::well_known::ext_staticlib_filename("perry_ext_http", None),
        )],
        None,
        OutputFormat::Json,
        0,
        &["external-http-client-pump"],
        &[],
    );
    match old_root {
        Some(root) => std::env::set_var("PERRY_WORKSPACE_ROOT", root),
        None => std::env::remove_var("PERRY_WORKSPACE_ROOT"),
    }
    let built = built.expect("HTTP rebuild must preserve runtime default features");
    assert!(built.stdlib.is_file() && built.runtime.is_file());
    assert_eq!(built.ext_libs.len(), 1);
    assert!(built.ext_libs.iter().all(|p| p.is_file()));
}

fn write_runtime_static_crate(root: &Path) {
    write_file(
        &root.join("crates/perry-runtime-static/Cargo.toml"),
        br#"
[package]
name = "perry-runtime-static"
version = "0.1.0"
edition = "2021"
[lib]
name = "perry_runtime"
crate-type = ["staticlib"]
[dependencies]
perry-runtime = { path = "../perry-runtime" }
"#,
    );
    write_file(
        &root.join("crates/perry-runtime-static/src/lib.rs"),
        b"pub use perry_runtime::*;\n",
    );
}

// Follow-up to #11225 / #11240: with auto-optimize off, importing `node:http`
// rebuilt only the stdlib and ext-http. Every other wrapper (ext-net here)
// and the runtime archive stayed prebuilt, from a different cargo graph, so
// the binary held two perry-ffi copies (two handle-id pools) and a runtime
// split from the one bundled into the stdlib. Every archive that bundles
// perry-ffi or perry-runtime must come out of the one invocation.
//
// The miniature ext-net refuses to compile unless perry-ffi was unified with
// the stdlib's pump feature — which is only true inside the shared graph —
// and decoy prebuilt archives on PERRY_LIB_DIR must not be linked.
#[test]
fn no_auto_http_rebuild_builds_every_linked_archive_in_one_graph() {
    let _guard = env_lock();
    let workspace = tempfile::tempdir().expect("workspace");
    let root = workspace.path();
    std::fs::create_dir_all(root.join("crates/perry-ui-geisterhand")).expect("workspace marker");
    write_file(
        &root.join("Cargo.toml"),
        br#"
[workspace]
resolver = "2"
members = ["crates/perry-ffi", "crates/perry-runtime", "crates/perry-runtime-static", "crates/perry-stdlib", "crates/perry-stdlib-static", "crates/perry-ext-http", "crates/perry-ext-net"]
"#,
    );
    write_file(
        &root.join("crates/perry-ffi/Cargo.toml"),
        br#"
[package]
name = "perry-ffi"
version = "0.1.0"
edition = "2021"
[features]
pump = []
"#,
    );
    write_file(
        &root.join("crates/perry-ffi/src/lib.rs"),
        b"pub const UNIFIED_WITH_STDLIB: bool = cfg!(feature = \"pump\");\n",
    );
    write_file(
        &root.join("crates/perry-runtime/Cargo.toml"),
        br#"
[package]
name = "perry-runtime"
version = "0.1.0"
edition = "2021"
[features]
default = []
stdlib = []
"#,
    );
    write_file(
        &root.join("crates/perry-runtime/src/lib.rs"),
        b"pub fn runtime_probe() -> u32 { 42 }\n",
    );
    write_runtime_static_crate(root);
    write_file(
        &root.join("crates/perry-stdlib/Cargo.toml"),
        br#"
[package]
name = "perry-stdlib"
version = "0.1.0"
edition = "2021"
[dependencies]
perry-ffi = { path = "../perry-ffi" }
[features]
external-http-client-pump = ["perry-ffi/pump"]
"#,
    );
    write_file(&root.join("crates/perry-stdlib/src/lib.rs"), b"");
    write_file(
        &root.join("crates/perry-stdlib-static/Cargo.toml"),
        br#"
[package]
name = "perry-stdlib-static"
version = "0.1.0"
edition = "2021"
[lib]
name = "perry_stdlib"
crate-type = ["staticlib"]
[dependencies]
perry-runtime = { path = "../perry-runtime", default-features = false, features = ["stdlib"] }
perry-stdlib = { path = "../perry-stdlib" }
"#,
    );
    write_file(
        &root.join("crates/perry-stdlib-static/src/lib.rs"),
        b"#[no_mangle]\npub extern \"C\" fn stdlib_probe() -> u32 { perry_runtime::runtime_probe() }\n",
    );
    for (krate, lib) in [
        ("perry-ext-http", "perry_ext_http"),
        ("perry-ext-net", "perry_ext_net"),
    ] {
        write_file(
            &root.join(format!("crates/{krate}/Cargo.toml")),
            format!(
                "[package]\nname = \"{krate}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
                 [lib]\nname = \"{lib}\"\ncrate-type = [\"staticlib\"]\n\
                 [dependencies]\nperry-ffi = {{ path = \"../perry-ffi\" }}\n"
            )
            .as_bytes(),
        );
    }
    write_file(&root.join("crates/perry-ext-http/src/lib.rs"), b"");
    write_file(
        &root.join("crates/perry-ext-net/src/lib.rs"),
        b"const _: () = assert!(perry_ffi::UNIFIED_WITH_STDLIB, \"ext-net built outside the stdlib's cargo graph\");\n",
    );

    // Decoy prebuilt wrappers: a mixed link would pick these up.
    let prebuilt = tempfile::tempdir().expect("prebuilt dir");
    let net = super::super::super::well_known::lookup_well_known("net").expect("net binding");
    let http = super::super::super::well_known::lookup_well_known("http").expect("http binding");
    let decoys: Vec<_> = [net, http]
        .iter()
        .map(|b| {
            prebuilt
                .path()
                .join(super::super::super::well_known::ext_staticlib_filename(
                    &b.lib,
                    rust_target_triple(None),
                ))
        })
        .collect();
    for decoy in &decoys {
        std::fs::write(decoy, b"!<arch>\n").expect("write decoy archive");
    }

    let saved: Vec<_> = [
        "PERRY_WORKSPACE_ROOT",
        "PERRY_LIB_DIR",
        "PERRY_RUNTIME_DIR",
        "PERRY_DISABLE_WELL_KNOWN",
        "CARGO_TARGET_DIR",
    ]
    .iter()
    .map(|k| (*k, std::env::var(k).ok()))
    .collect();
    set_env_var("PERRY_WORKSPACE_ROOT", root.to_str());
    set_env_var("PERRY_LIB_DIR", prebuilt.path().to_str());
    set_env_var("PERRY_RUNTIME_DIR", None);
    set_env_var("PERRY_DISABLE_WELL_KNOWN", None);
    set_env_var("CARGO_TARGET_DIR", None);

    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.native_module_imports.insert("http".to_string());
    ctx.native_module_imports.insert("net".to_string());
    let libs = resolve_no_auto_optimized_libs(&ctx, None, OutputFormat::Json, 0);

    for (key, value) in &saved {
        set_env_var(key, value.as_deref());
    }

    let stdlib = libs
        .stdlib
        .as_ref()
        .expect("http import rebuilds the stdlib");
    let graph_dir = stdlib.parent().expect("stdlib archive dir").to_path_buf();
    let runtime = libs
        .runtime
        .as_ref()
        .expect("the runtime archive must come from the rebuilt graph, not the prebuilt one");
    assert_eq!(
        runtime.parent(),
        Some(graph_dir.as_path()),
        "runtime {runtime:?}"
    );
    for decoy in &decoys {
        assert!(
            !libs.well_known_libs.contains(decoy),
            "prebuilt {decoy:?} linked beside the rebuilt stdlib: {:?}",
            libs.well_known_libs
        );
    }
    for stem in [&net.lib, &http.lib] {
        let name =
            super::super::super::well_known::ext_staticlib_filename(stem, rust_target_triple(None));
        let expected = graph_dir.join(&name);
        assert!(
            libs.well_known_libs.contains(&expected) && expected.is_file(),
            "expected {expected:?} from the shared graph, got {:?}",
            libs.well_known_libs
        );
    }
}
