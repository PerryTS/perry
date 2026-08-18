//! Regression coverage for Test262 tracker #5902.
//!
//! A member update performs GetValue, ToNumeric, the numeric step, and then
//! PutValue. The final write may be rejected by [[Set]]. In sloppy scripts that
//! rejection is a silent no-op; in strict code it throws a TypeError. Perry's
//! update HIR previously lost the source strictness and always used the
//! throwing by-name setter for `object.property++`. The same Test262 case also
//! reaches `with (boxedString) length = value`; that helper accepted a strict
//! flag but bypassed the strict-aware setter.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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

fn runtime_dir() -> PathBuf {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        // `cargo test` rebuilds the compiler and this test binary, but it does
        // not build staticlib targets. Compile the runtime archive explicitly
        // so the fixture cannot link a stale pre-fix `js_with_set_binding`
        // from an earlier checkout of the same target directory.
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .output()
            .expect("build static runtime archive");
        assert!(
            build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });

    perry_bin()
        .parent()
        .expect("Perry binary directory")
        .to_path_buf()
}

fn compile_js(dir: &Path, name: &str, source: &str) -> PathBuf {
    let entry = dir.join(format!("{name}.js"));
    let output = dir.join(format!("{name}_bin"));
    std::fs::write(&entry, source).expect("write JavaScript fixture");

    let perry = perry_bin();
    let runtime_dir = runtime_dir();
    let compile = Command::new(&perry)
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", &runtime_dir)
        .env("PERRY_LIB_DIR", &runtime_dir)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run(binary: &Path) -> Output {
    Command::new(binary).output().expect("run compiled binary")
}

#[test]
fn sloppy_member_updates_ignore_rejected_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile_js(
        dir.path(),
        "sloppy",
        r#"
var frozen = Object.freeze({ named: 1, computed: 2 });
var namedResult = frozen.named++;
var computedResult = --frozen["computed"];

var boxed = new String("globglob");
with (boxed) length = 0;
var lengthResult = boxed.length++;

var descriptor = {};
Object.defineProperty(descriptor, "value", { value: 4, writable: false });
var descriptorResult = ++descriptor.value;

console.log("named", namedResult, frozen.named);
console.log("computed", computedResult, frozen.computed);
console.log("boxed", lengthResult, boxed.length);
console.log("descriptor", descriptorResult, descriptor.value);
"#,
    );

    let output = run(&binary);
    assert!(
        output.status.success(),
        "sloppy updates must not throw\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "named 1 1\ncomputed 1 2\nboxed 8 8\ndescriptor 5 4\n"
    );
}

#[test]
fn strict_member_update_still_throws_on_a_rejected_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile_js(
        dir.path(),
        "strict",
        r#"
"use strict";
var frozen = Object.freeze({ value: 1 });
frozen.value++;
"#,
    );

    let output = run(&binary);
    assert!(
        !output.status.success(),
        "strict update unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("read only property 'value'"),
        "strict update did not report the rejected write\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
