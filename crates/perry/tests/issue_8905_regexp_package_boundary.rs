//! Regression test for #8905: a RegExp passed into a `compilePackages`
//! dependency must retain RegExp method behavior when the dependency reads it
//! back through an object property.

use std::path::PathBuf;
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

fn ensure_runtime_archives() {
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

#[test]
fn regexp_methods_survive_the_compiled_package_boundary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "regexp-package-boundary",
  "private": true,
  "type": "module",
  "perry": {
    "compilePackages": ["regex-consumer"],
    "allow": { "compilePackages": ["regex-consumer"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let package = root.join("node_modules/regex-consumer");
    std::fs::create_dir_all(&package).expect("mkdir regex-consumer");
    std::fs::write(
        package.join("package.json"),
        r#"{
  "name": "regex-consumer",
  "version": "1.0.0",
  "type": "module",
  "exports": "./index.js"
}"#,
    )
    .expect("write dependency package.json");
    std::fs::write(
        package.join("index.js"),
        r#"
import { randomUUID } from "node:crypto";

export function makeRegexCheck(def) {
  return (value) => {
    def.pattern.lastIndex = 0;
    return def.pattern.test(value);
  };
}

export function stdlibMarker() {
  return typeof randomUUID;
}
"#,
    )
    .expect("write compiled dependency");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { makeRegexCheck, stdlibMarker } from "regex-consumer";

const check = makeRegexCheck({ pattern: /^a+$/ });
console.log(stdlibMarker(), check("aaa"), check("bbb"));
"#,
    )
    .expect("write entry");

    // The node:crypto import forces the full-stdlib link used by the reporter.
    // With PERRY_NO_AUTO_OPTIMIZE that can put a second statically linked
    // runtime on the compiled-package side of this RegExp method call.
    ensure_runtime_archives();
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "function true false\n"
    );
}
