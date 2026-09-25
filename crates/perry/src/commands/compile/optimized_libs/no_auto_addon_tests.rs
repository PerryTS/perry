//! Exercise the real no-auto resolver against a tiny Cargo workspace. Each
//! linked archive must see Node-API, optional Wasm, and the HTTP pump together.
use super::*;
use crate::commands::compile::NativeAddonModule;
use std::ffi::OsString;

struct SavedEnv(Vec<(&'static str, Option<OsString>)>);
impl SavedEnv {
    fn set(values: &[(&'static str, Option<&std::ffi::OsStr>)]) -> Self {
        let saved = values
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in values {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self(saved)
    }
}
impl Drop for SavedEnv {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
fn krate(root: &Path, name: &str, options: &str, source: &str) {
    let dir = root.join("crates").join(name);
    write(
        &dir.join("Cargo.toml"),
        &format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2021\"\n{options}"),
    );
    write(&dir.join("src/lib.rs"), source);
}

#[cfg(unix)]
fn check_addon_graph(http: bool, wasm: bool) {
    let _lock = crate::test_env_lock::env_lock();
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path();
    std::fs::create_dir_all(root.join("crates/perry-ui-geisterhand")).unwrap();
    write(&root.join("Cargo.toml"), "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\nexclude = [\"crates/perry-ui-geisterhand\"]\n");
    krate(root, "perry-runtime", "[features]\ndefault = [\"regex-engine\"]\nregex-engine = []\nstdlib = []\nnode-api-host = []\nwasm-host = []\n", r#"
#[cfg(not(feature = "regex-engine"))]
compile_error!("runtime defaults were lost");
pub const HOST: bool = cfg!(feature = "node-api-host");
pub const WASM: bool = cfg!(feature = "wasm-host");
#[no_mangle] pub extern "C" fn runtime_probe() -> i32 { if HOST { 42 } else { 0 } }
"#);
    krate(
        root,
        "perry-ffi",
        "[features]\npump = []\n",
        "pub const PUMP: bool = cfg!(feature = \"pump\");",
    );
    krate(root, "perry-stdlib", "[dependencies]\nperry-ffi = { path = \"../perry-ffi\" }\n[features]\nexternal-http-client-pump = [\"perry-ffi/pump\"]\n", "pub use perry_ffi::PUMP;");
    krate(root, "perry-runtime-static", "[lib]\nname = \"perry_runtime\"\ncrate-type = [\"staticlib\"]\n[dependencies]\nperry-runtime = { path = \"../perry-runtime\" }\n", "pub use perry_runtime::*;");
    let checks = format!("const _: () = assert!(perry_runtime::HOST, \"Node-API disabled in linked archive\");\nconst _: () = assert!(perry_runtime::WASM == {wasm});\n");
    krate(root, "perry-stdlib-static", "[lib]\nname = \"perry_stdlib\"\ncrate-type = [\"staticlib\"]\n[dependencies]\nperry-runtime = { path = \"../perry-runtime\", default-features = false, features = [\"stdlib\"] }\nperry-stdlib = { path = \"../perry-stdlib\" }\n", &format!("{checks}\nconst _: () = assert!(perry_stdlib::PUMP == {http});\n#[no_mangle] pub extern \"C\" fn stdlib_probe() -> i32 {{ perry_runtime::runtime_probe() }}"));
    for name in ["net", "http"] {
        krate(root, &format!("perry-ext-{name}"), &format!("[lib]\nname = \"perry_ext_{name}\"\ncrate-type = [\"staticlib\"]\n[dependencies]\nperry-runtime = {{ path = \"../perry-runtime\", default-features = false }}\nperry-ffi = {{ path = \"../perry-ffi\" }}\n"), &format!("{checks}\nconst _: () = assert!(perry_ffi::PUMP == {http});\n#[no_mangle] pub extern \"C\" fn {name}_probe() -> i32 {{ perry_runtime::runtime_probe() }}"));
    }
    let prebuilt = tempfile::tempdir().unwrap();
    let mut imports = vec!["net"];
    if http {
        imports.push("http");
    }
    let decoys: Vec<_> = imports
        .iter()
        .map(|name| {
            let binding = super::super::super::well_known::lookup_well_known(name).unwrap();
            let path =
                prebuilt
                    .path()
                    .join(super::super::super::well_known::ext_staticlib_filename(
                        &binding.lib,
                        rust_target_triple(None),
                    ));
            std::fs::write(&path, b"!<arch>\n").unwrap();
            path
        })
        .collect();
    let _env = SavedEnv::set(&[
        ("PERRY_WORKSPACE_ROOT", Some(root.as_os_str())),
        ("PERRY_LIB_DIR", Some(prebuilt.path().as_os_str())),
        ("PERRY_RUNTIME_DIR", None),
        ("PERRY_DISABLE_WELL_KNOWN", None),
        ("CARGO_TARGET_DIR", None),
    ]);
    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.needs_stdlib = true;
    ctx.needs_wasm_runtime = wasm;
    ctx.native_module_imports
        .extend(imports.iter().map(|s| s.to_string()));
    ctx.native_addons.insert(
        "demo/addon.node".into(),
        NativeAddonModule {
            logical_id: "demo/addon.node".into(),
            package: "demo".into(),
            version: "1.0.0".into(),
            source_path: root.join("addon.node"),
            package_dir: root.to_path_buf(),
            entry_relative: "addon.node".into(),
            ship_package_payload: false,
        },
    );
    let libs = resolve_no_auto_optimized_libs(&ctx, None, OutputFormat::Json, 0);
    let stdlib = libs
        .stdlib
        .expect("addon rebuild must supply its feature-matched stdlib");
    let runtime = libs.runtime.expect("addon runtime");
    let graph = stdlib.parent().unwrap();
    assert_eq!(runtime.parent(), Some(graph));
    assert_eq!(libs.well_known_libs.len(), imports.len());
    for path in &libs.well_known_libs {
        assert_eq!(path.parent(), Some(graph));
        assert!(
            !decoys.contains(path),
            "prebuilt wrapper survived the rebuild"
        );
    }
    let c = root.join("probe.c");
    write(&c, &format!("int stdlib_probe(void); int runtime_probe(void); int net_probe(void); int http_probe(void);\nint main(void) {{ return stdlib_probe()!=42 || runtime_probe()!=42 || net_probe()!=42 {}; }}\n", if http { "|| http_probe()!=42" } else { "" }));
    let executable = root.join("probe");
    let mut cc = Command::new("cc");
    cc.arg(&c)
        .arg(&stdlib)
        .args(&libs.well_known_libs)
        .arg(&runtime)
        .args(["-lpthread", "-ldl", "-lm", "-o"])
        .arg(&executable);
    let output = cc.output().unwrap();
    assert!(
        output.status.success(),
        "native link: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        Command::new(executable).status().unwrap().success(),
        "stdlib-first link selected a non-host runtime"
    );
}

#[test]
#[cfg(unix)]
fn addon_stdlib_and_wrappers_share_host_features() {
    check_addon_graph(false, false);
}
#[test]
#[cfg(unix)]
fn addon_http_keeps_the_client_pump_and_host_features() {
    check_addon_graph(true, false);
}
#[test]
#[cfg(unix)]
fn addon_http_and_wasm_share_all_requested_features() {
    check_addon_graph(true, true);
}
