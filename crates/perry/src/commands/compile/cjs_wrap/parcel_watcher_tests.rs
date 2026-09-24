use super::*;
use std::path::PathBuf;

#[test]
fn wrap_folds_opencode_parcel_watcher_template_require_for_target() {
    let src = r#"
const libc = typeof OPENCODE_LIBC === "undefined" ? undefined : OPENCODE_LIBC
const binding = require(
  `@parcel/watcher-${process.platform}-${process.arch}${process.platform === "linux" ? `-${libc || "glibc"}` : ""}`,
)
module.exports = binding
"#;
    let wrapped = wrap_commonjs_for_target(
        src,
        &PathBuf::from("/tmp/node_modules/opencode/watcher.js"),
        Some("linux-x86_64-musl"),
        false,
        None,
    );
    assert!(
        wrapped.contains("from '@parcel/watcher-linux-x64-musl'")
            || wrapped.contains("from \"@parcel/watcher-linux-x64-musl\""),
        "target-specific sidecar must become a static import:\n{wrapped}"
    );
    assert!(!wrapped.contains("require(\n  `@parcel/watcher-"));
}

#[test]
fn explicit_and_wildcard_compile_packages_preserve_watcher_facades() {
    for package in ["@parcel/watcher", "@parcel/watcher-darwin-arm64"] {
        let dir = tempfile::tempdir().unwrap();
        let package_dir = dir.path().join("node_modules").join(package);
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(
            package_dir.join("package.json"),
            serde_json::json!({"name": package, "version": "2.5.1", "main": "watcher.node"})
                .to_string(),
        )
        .unwrap();
        std::fs::write(package_dir.join("watcher.node"), b"native facade marker").unwrap();
        let source = format!("const watcher = require('{package}'); watcher.writeSnapshot('tree', 'snapshot', {{}});");
        for selection in [package, "*"] {
            let packages = std::collections::HashSet::from([selection.to_string()]);
            let wrapped = wrap_commonjs_for_target(
                &source,
                &dir.path().join("main.js"),
                None,
                true,
                Some(&packages),
            );
            assert!(
                wrapped.contains(&format!("from '{package}'"))
                    || wrapped.contains(&format!("from \"{package}\"")),
                "{package} under {selection} must retain its facade import:\n{wrapped}"
            );
            assert!(!wrapped.contains("/watcher.node"), "{wrapped}");
        }
    }
}

#[test]
fn watcher_javascript_wrapper_still_resolves_from_source() {
    let dir = tempfile::tempdir().unwrap();
    let package_dir = dir.path().join("node_modules/@parcel/watcher");
    std::fs::create_dir_all(&package_dir).unwrap();
    std::fs::write(
        package_dir.join("package.json"),
        r#"{"name":"@parcel/watcher","version":"2.5.1","main":"index.js"}"#,
    )
    .unwrap();
    std::fs::write(package_dir.join("wrapper.js"), "module.exports = {};\n").unwrap();
    let packages = std::collections::HashSet::from(["@parcel/watcher".to_string()]);
    let wrapped = wrap_commonjs_for_target(
        "module.exports = require('@parcel/watcher/wrapper');",
        &dir.path().join("main.js"),
        None,
        true,
        Some(&packages),
    );
    let import_line = wrapped
        .lines()
        .find(|line| line.starts_with("import _req_0 from "))
        .expect("hoisted wrapper import");
    assert!(
        import_line.contains("/node_modules/@parcel/watcher/wrapper.js"),
        "{import_line}"
    );
}
