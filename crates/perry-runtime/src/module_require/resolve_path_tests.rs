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
