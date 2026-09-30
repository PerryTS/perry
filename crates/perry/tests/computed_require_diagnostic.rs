//! Computed CommonJS dependencies produce actionable original-source diagnostics.
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn computed_require_warning_names_source_and_static_dependency_workaround() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("dep.js"), "module.exports = 42;").unwrap();
    let loader = root.join("loader.cjs");
    std::fs::write(&loader, "const r = require;\nrequire('./dep.js');\nexports.load = name => r('./' + name + '.js');\n").unwrap();
    let entry = root.join("main.cjs");
    std::fs::write(
        &entry,
        "const loader = require('./loader.cjs'); console.log(loader.load('dep'));\n",
    )
    .unwrap();
    let oracle = Command::new("node").arg(&entry).output().unwrap();
    assert!(oracle.status.success());
    assert_eq!(oracle.stdout, b"42\n");
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_path_buf());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let binary = root.join(if cfg!(windows) { "app.exe" } else { "app" });
    let compile = Command::new(compiler)
        .env("PERRY_RUNTIME_DIR", runtime)
        .env("PERRY_WORKSPACE_ROOT", workspace)
        .args(["compile", "--no-cache", "--no-auto-optimize"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    let diagnostics = String::from_utf8_lossy(&compile.stderr);
    assert!(compile.status.success(), "{diagnostics}");
    assert!(
        diagnostics.contains(&format!(
            "{}:3:24: computed CommonJS require",
            loader.display()
        )),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("add static imports for the possible targets and use explicit file extensions in computed requests (#10438)"),
        "{diagnostics}"
    );
    assert_eq!(
        diagnostics.matches("computed CommonJS require").count(),
        1,
        "{diagnostics}"
    );
    let native = Command::new(binary).output().unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(native.stdout, oracle.stdout);
}
