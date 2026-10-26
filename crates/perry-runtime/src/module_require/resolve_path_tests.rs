use super::*;
use std::path::{Path, PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "perry-require-resolve-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn resolve(from: &Path, specifier: &str) -> Option<String> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let from = scope.root_nanbox_f64(string_value(&from.to_string_lossy()));
    let specifier = scope.root_nanbox_f64(string_value(specifier));
    let result = js_require_resolve_node_modules(from.get_nanbox_f64(), specifier.get_nanbox_f64());
    (!JSValue::from_bits(result.to_bits()).is_undefined())
        .then(|| value_to_string(result, "resolved"))
}

#[test]
fn require_resolve_relative_files_use_the_requesting_module_directory() {
    let fixture = Fixture::new();
    let base = fixture.0.join("app");
    std::fs::create_dir_all(base.join("folder")).unwrap();
    for name in [
        "app/m.js",
        "app/index.js",
        "app/folder/index.js",
        "shared.json",
        "index.js",
    ] {
        std::fs::write(fixture.0.join(name), "{}").unwrap();
    }
    for (specifier, target) in [
        ("./m.js", "app/m.js"),
        ("./m", "app/m.js"),
        ("./folder", "app/folder/index.js"),
        ("./folder/../m.js", "app/m.js"),
        ("../shared", "shared.json"),
        (".", "app/index.js"),
        ("..", "index.js"),
    ] {
        assert_eq!(
            resolve(&base, specifier),
            Some(
                fixture
                    .0
                    .join(target)
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            ),
            "{specifier}"
        );
    }
    assert_eq!(resolve(&base, "./missing.js"), None);
    assert_eq!(resolve(&base, ""), None);
}

#[cfg(unix)]
#[test]
fn require_resolve_relative_files_canonicalize_symlinks() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("real.js"), "module.exports = 1;").unwrap();
    std::os::unix::fs::symlink("real.js", fixture.0.join("alias.js")).unwrap();
    assert_eq!(
        resolve(&fixture.0, "./alias.js"),
        Some(
            fixture
                .0
                .join("real.js")
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        )
    );
}

struct RegisteredFiles(Vec<String>);
impl Drop for RegisteredFiles {
    fn drop(&mut self) {
        MODULE_PATH_REGISTRY.with(|registry| {
            for key in &self.0 {
                registry.remove_for_test(key);
            }
        });
    }
}

#[test]
fn compiled_modules_resolve_without_build_host_files() {
    let _lock = crate::gc::global_side_table_test_lock();
    let fixture = Fixture::new();
    let base = fixture.0.canonicalize().unwrap();
    let keys: Vec<_> = ["gone/db.json", "gone/code.js", "gone/folder/index.json"]
        .into_iter()
        .map(|path| base.join(path).to_string_lossy().into_owned())
        .collect();
    let _registered = RegisteredFiles(keys.clone());
    for key in &keys {
        assert!(!Path::new(key).exists());
        // Undefined is a valid export, so resolution needs a presence test.
        assert!(MODULE_PATH_REGISTRY.with(|registry| {
            registry.register_final_exports(key.clone(), crate::value::TAG_UNDEFINED)
        }));
    }
    for (request, target) in [
        ("./gone/db.json", "gone/db.json"),
        ("./gone/db", "gone/db.json"),
        ("./gone/sub/../db.json", "gone/db.json"),
        ("./gone/code", "gone/code.js"),
        ("./gone/folder", "gone/folder/index.json"),
    ] {
        assert_eq!(
            resolve_request(&base, request).ok(),
            Some(base.join(target)),
            "{request} must resolve from the compiled registry"
        );
    }
    assert!(resolve_request(&base, "./gone/missing.json").is_err());
}

extern "C" fn verbatim_key_test_init() {}

/// The compiler emits each module's key already canonical, so registration
/// stores it verbatim and never asks the filesystem; only run-time spellings
/// canonicalize, and they meet the same key.
#[cfg(unix)]
#[test]
fn registered_keys_are_verbatim_and_runtime_spellings_meet_them() {
    let _lock = crate::gc::global_side_table_test_lock();
    let fixture = Fixture::new();
    let base = fixture.0.canonicalize().unwrap();
    std::fs::create_dir_all(base.join("real/sub")).unwrap();
    std::fs::write(base.join("real/m.js"), "module.exports = 1;").unwrap();
    std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();

    // Registration does not canonicalize: a key spelled through a symlink
    // stays that spelling (the old registration resolved it to `real/`).
    let through_link = base
        .join("link/registered.js")
        .to_string_lossy()
        .into_owned();
    unsafe {
        js_register_path_init(
            through_link.as_ptr(),
            through_link.len() as i64,
            verbatim_key_test_init as *const () as usize as i64,
        );
    }
    let resolved = base
        .join("real/registered.js")
        .to_string_lossy()
        .into_owned();
    MODULE_PATH_REGISTRY.with(|registry| {
        assert!(registry.contains_registered(&through_link));
        assert!(!registry.contains_registered(&resolved));
    });

    // A canonical key, as the compiler emits it, published like a CJS wrapper.
    let key = base.join("real/m.js").to_string_lossy().into_owned();
    let _registered = RegisteredFiles(vec![key.clone()]);
    assert!(
        MODULE_PATH_REGISTRY
            .with(|registry| registry
                .register_final_exports(key.clone(), crate::value::TAG_UNDEFINED))
    );
    // The key itself names the module; aliases computed at run time
    // canonicalize to the same key (one spelling per module, #10851).
    for spelling in [
        key.clone(),
        base.join("link/m.js").to_string_lossy().into_owned(),
        base.join("real/sub/../m.js").to_string_lossy().into_owned(),
    ] {
        assert_eq!(module_lookup_key(&spelling), key, "{spelling}");
    }

    // A key whose build-host directory does not exist on this machine (the
    // binary runs elsewhere) is still found by its own spelling.
    let foreign = base.join("gone/dir/x.js").to_string_lossy().into_owned();
    let _foreign = RegisteredFiles(vec![foreign.clone()]);
    assert!(MODULE_PATH_REGISTRY.with(|registry| {
        registry.register_final_exports(foreign.clone(), crate::value::TAG_UNDEFINED)
    }));
    assert_eq!(module_lookup_key(&foreign), foreign);
}

extern "C" fn lazy_path_init_test_init() {}

const LAZY_KEY: &str = "/perry-lazy-path-init-test/on-demand.js";

unsafe extern "C" fn lazy_path_init_test_registrar() {
    js_register_path_init(
        LAZY_KEY.as_ptr(),
        LAZY_KEY.len() as i64,
        lazy_path_init_test_init as *const () as usize as i64,
    );
}

/// CJL: an on-demand CommonJS module's initializer is registered only when a
/// runtime path lookup first needs one, not at startup.
#[test]
fn a_lazy_registrar_runs_at_the_first_path_lookup() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe { js_register_path_inits_lazily(lazy_path_init_test_registrar) };
    let registered =
        || MODULE_PATH_REGISTRY.with(|registry| registry.contains_registered(LAZY_KEY));
    assert!(!registered(), "registered before any lookup");
    let _ = module_lookup_key(LAZY_KEY);
    assert!(registered(), "the first lookup runs the registrar");
    MODULE_PATH_REGISTRY.with(|registry| registry.remove_for_test(LAZY_KEY));
}
