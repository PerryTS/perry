//! Regression for #8516: two installed versions of one package must remain
//! separate native modules and resolve relative to their respective importers.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if cfg!(windows) {
        target.join("x86_64-pc-windows-msvc").join("debug")
    } else {
        target.join("debug")
    }
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static");
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let build = command.output().expect("build static runtime archives");
        assert!(
            build.status.success(),
            "runtime archive build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn write(path: &Path, source: &str) {
    std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("mkdir fixture");
    std::fs::write(path, source).expect("write fixture");
}

fn write_package(root: &Path, name: &str, version: &str, source: &str) {
    write(
        &root.join("package.json"),
        &format!(
            r#"{{"name":"{name}","version":"{version}","type":"module","exports":"./index.js"}}"#
        ),
    );
    write(&root.join("index.js"), source);
}

#[test]
fn nested_versions_match_importer_relative_node_resolution() {
    let fixture = tempfile::tempdir().expect("tempdir");
    let root = fixture.path();
    write(
        &root.join("package.json"),
        r#"{
          "name": "package-instance-fixture",
          "type": "module",
          "perry": {
            "compilePackages": "auto",
            "allow": { "compilePackages": "auto" }
          }
        }"#,
    );
    write_package(
        &root.join("node_modules/dup-pkg"),
        "dup-pkg",
        "1.0.0",
        "export function identify() { return 'top-v1'; }\n",
    );
    write_package(
        &root.join("node_modules/holder/node_modules/dup-pkg"),
        "dup-pkg",
        "2.0.0",
        "export function identify() { return 'nested-v2'; }\n",
    );
    write_package(
        &root.join("node_modules/holder"),
        "holder",
        "1.0.0",
        "import { identify } from 'dup-pkg';\nexport function child() { return identify(); }\n",
    );
    write(
        &root.join("main.ts"),
        "import { identify } from 'dup-pkg';\n\
         import { child } from 'holder';\n\
         console.log(identify(), child());\n",
    );

    ensure_runtime_archive();
    let binary = root.join(if cfg!(windows) { "main.exe" } else { "main" });
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg("main.ts")
        .arg("--no-cache")
        .arg("-o")
        .arg(&binary)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir())
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(
        !diagnostics.contains("ONE copy per package name"),
        "the obsolete one-copy warning must be gone:\n{diagnostics}"
    );

    let run = Command::new(&binary)
        .current_dir(root)
        .output()
        .expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "top-v1 nested-v2\n");
}
