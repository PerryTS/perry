use super::*;
use std::collections::BTreeMap;

#[test]
fn project_addon_require_and_resolve_share_the_authorized_id() {
    for manifest in [r#"{"name":"named-project"}"#, r#"{"private":true}"#] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), manifest).unwrap();
        let native = dir.path().join("native");
        std::fs::create_dir(&native).unwrap();
        let addon = native.join("addon.node");
        std::fs::write(&addon, []).unwrap();
        let paths = BTreeMap::from([(addon.canonicalize().unwrap(), "native/addon.node".into())]);
        let source = "module.exports = { addon: require('./addon.node'), id: require.resolve('./addon.node') };";
        let (wrapped, offset) = wrap_commonjs_with_addon_paths(
            source,
            &native.join("index.cjs"),
            None,
            false,
            None,
            Some(&paths),
        );
        assert!(
            wrapped.contains("process.dlopen(nativeModule, \"$project/native/addon.node\")"),
            "{manifest}: {wrapped}"
        );
        assert!(
            wrapped.contains(
                "if (specifier === \"./addon.node\") return \"$project/native/addon.node\";"
            ),
            "{wrapped}"
        );
        assert!(offset.is_some(), "debug source mapping is retained");
    }
}

#[test]
fn exact_project_mapping_wins_over_nested_package_name() {
    let dir = tempfile::tempdir().unwrap();
    let native = dir.path().join("native");
    std::fs::create_dir(&native).unwrap();
    std::fs::write(native.join("package.json"), r#"{"name":"nested-vendor"}"#).unwrap();
    let addon = native.join("addon.node");
    std::fs::write(&addon, []).unwrap();
    let paths = BTreeMap::from([(addon.canonicalize().unwrap(), "native/addon.node".into())]);
    let (wrapped, _) = wrap_commonjs_with_addon_paths(
        "module.exports = require('./native/../native/addon.node');",
        &dir.path().join("index.cjs"),
        None,
        false,
        None,
        Some(&paths),
    );
    assert!(
        wrapped.contains("process.dlopen(nativeModule, \"$project/native/addon.node\")"),
        "{wrapped}"
    );
    assert!(!wrapped.contains("nested-vendor/addon.node"));
}

#[test]
fn package_addon_keeps_its_package_logical_id() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("node_modules/demo-addon");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("package.json"), r#"{"name":"demo-addon"}"#).unwrap();
    std::fs::write(package.join("addon.node"), []).unwrap();
    let (wrapped, _) = wrap_commonjs_with_addon_paths(
        "module.exports = require('./addon.node');",
        &package.join("index.cjs"),
        None,
        false,
        None,
        Some(&BTreeMap::new()),
    );
    assert!(
        wrapped.contains("process.dlopen(nativeModule, \"demo-addon/addon.node\")"),
        "{wrapped}"
    );
}
