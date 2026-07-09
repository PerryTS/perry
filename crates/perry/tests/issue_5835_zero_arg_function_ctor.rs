//! #5835 regression: a ZERO-argument `new Function()` must construct the empty
//! function `anonymous() {}`, never the CSP "dynamic code generation is
//! unavailable" throw.
//!
//! The throw exists to make the zod-style codegen capability probe
//! (`new Function("")`, which passes an argument) fail so zod takes its
//! non-codegen interpreter fallback. It was firing for ARGUMENT-LESS
//! `new Function()` too — which is not a codegen request at all, just an empty
//! constructable — and regressed `test_gap_intl_ctor_mechanics_5835` (which
//! uses `new Function()` as a settable-prototype scaffold for
//! `Reflect.construct`). The refusal is now gated on there being ≥1 argument.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run(dir: &std::path::Path, src: &str) -> String {
    let entry = dir.join("main.ts");
    let out = dir.join("main_bin");
    std::fs::write(&entry, src).expect("write");
    let c = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&out)
        .arg("--no-cache")
        .output()
        .expect("compile");
    assert!(
        c.status.success(),
        "compile failed\n{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let r = Command::new(&out).current_dir(dir).output().expect("run");
    assert!(
        r.status.success(),
        "run failed\n{}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8_lossy(&r.stdout).into_owned()
}

#[test]
fn zero_arg_new_function_is_an_empty_callable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        r#"
const f: any = new Function();
f.prototype = {};
console.log(typeof f, (() => { try { f(); return "ok"; } catch { return "threw"; } })(), typeof f.prototype);
"#,
    );
    assert_eq!(out, "function ok object\n");
}

#[test]
fn zero_arg_function_called_form_is_an_empty_callable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "const f: any = Function();\nconsole.log(typeof f);\n",
    );
    assert_eq!(out, "function\n");
}

/// The argument-bearing codegen probe must still be refused so zod-style JIT
/// feature-detection takes its non-codegen fallback (default CSP behavior).
#[test]
fn empty_string_arg_probe_still_throws() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        r#"
try { new Function(""); console.log("no-throw"); }
catch { console.log("threw"); }
"#,
    );
    assert_eq!(out, "threw\n");
}

/// A real literal body still folds and runs — the fix must not disturb the
/// spec-preserving `CreateDynamicFunction` path for statically-known sources.
#[test]
fn real_literal_body_still_folds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "const f: any = new Function(\"a\", \"b\", \"return a + b\");\nconsole.log(f(2, 3));\n",
    );
    assert_eq!(out, "5\n");
}
