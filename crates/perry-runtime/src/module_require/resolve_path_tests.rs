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
