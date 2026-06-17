//! Regression test for #5253 (follow-up to #5247/#5250): the runtime
//! source-location diagnostics that #5250 gave the dynamic call-dispatch
//! ("X is not a function") throw are extended to two more throw classes, both
//! gated on `--debug-symbols`:
//!
//!   1. `new X()` "X is not a constructor" — `const X: any = undefined;
//!      new X();` lowers to `Expr::NewDynamic` (callee `LocalGet`) and the
//!      runtime construct path throws a TypeError. This is the shape that
//!      localizes ajv's `undefined is not a constructor`.
//!   2. `ReferenceError: X is not defined` — a bare unresolved identifier read
//!      (`notDefinedAnywhere`) lowers to a call into
//!      `js_global_get_or_throw_unresolved`, which throws a ReferenceError.
//!      This is the shape that localizes winston's `module is not defined`.
//!
//! Behavior:
//!   • WITH `--debug-symbols`: the thrown error's `.stack` contains
//!     `at <file>:<line>` pointing at the offending line.
//!   • WITHOUT the flag (default build): unchanged — `at <anonymous>`.

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
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

/// Build `libperry_runtime.a` once so the compiled binaries can link.
fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime")
            .output()
            .expect("run cargo build -p perry-runtime");
        assert!(
            build.status.success(),
            "cargo build -p perry-runtime failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archive();
    target_debug_dir()
}

fn compile(root: &std::path::Path, extra_args: &[&str]) -> std::process::Output {
    let entry = root.join("main.ts");
    let output = root.join("main_bin");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.env("PERRY_NO_AUTO_OPTIMIZE", "1");
    cmd.env("PERRY_RUNTIME_DIR", runtime_dir());
    cmd.output().expect("run perry compile")
}

fn run_fixture(fixture: &str, extra_args: &[&str]) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), fixture).expect("write entry");

    let out = compile(root, extra_args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "compile must succeed (args {extra_args:?}); stderr:\n{stderr}"
    );

    let bin = root.join("main_bin");
    let run = Command::new(&bin).output().expect("run compiled binary");
    String::from_utf8_lossy(&run.stdout).into_owned()
}

// ---- 1. `new X()` not-a-constructor ----

/// `new X()` is on line 4 (1 = blank from the raw-string leading newline,
/// 2 = `const`, 3 = `try {`, 4 = `new X();`).
const NOT_A_CONSTRUCTOR_FIXTURE: &str = r#"
const X: any = undefined;
try {
  new X();
} catch (e: any) {
  console.log("MSG:" + e.message);
  console.log("STACK:" + e.stack);
}
"#;

#[test]
fn debug_symbols_attaches_file_line_to_not_a_constructor_throw() {
    let stdout = run_fixture(NOT_A_CONSTRUCTOR_FIXTURE, &["--debug-symbols"]);
    assert!(
        stdout.contains("MSG:") && stdout.contains("is not a constructor"),
        "expected a 'is not a constructor' TypeError; got:\n{stdout}"
    );
    assert!(
        stdout.contains("at main.ts:4"),
        "expected 'at main.ts:4' frame with --debug-symbols; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("<anonymous>"),
        "the location must replace the <anonymous> frame; got:\n{stdout}"
    );
}

#[test]
fn default_build_keeps_anonymous_frame_for_not_a_constructor() {
    let stdout = run_fixture(NOT_A_CONSTRUCTOR_FIXTURE, &[]);
    assert!(
        stdout.contains("is not a constructor"),
        "expected a 'is not a constructor' TypeError; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("at main.ts:"),
        "default build must NOT emit a source location; got:\n{stdout}"
    );
}

// ---- 2. ReferenceError `X is not defined` ----

/// The bare read of `notDefinedAnywhere` is on line 3 (1 = blank, 2 = `try {`,
/// 3 = the throwing read).
const REFERENCE_ERROR_FIXTURE: &str = r#"
try {
  console.log(notDefinedAnywhere);
} catch (e: any) {
  console.log("MSG:" + e.message);
  console.log("STACK:" + e.stack);
}
"#;

#[test]
fn debug_symbols_attaches_file_line_to_reference_error_throw() {
    let stdout = run_fixture(REFERENCE_ERROR_FIXTURE, &["--debug-symbols"]);
    assert!(
        stdout.contains("MSG:") && stdout.contains("is not defined"),
        "expected a 'is not defined' ReferenceError; got:\n{stdout}"
    );
    assert!(
        stdout.contains("at main.ts:3"),
        "expected 'at main.ts:3' frame with --debug-symbols; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("<anonymous>"),
        "the location must replace the <anonymous> frame; got:\n{stdout}"
    );
}

#[test]
fn default_build_keeps_anonymous_frame_for_reference_error() {
    let stdout = run_fixture(REFERENCE_ERROR_FIXTURE, &[]);
    assert!(
        stdout.contains("is not defined"),
        "expected a 'is not defined' ReferenceError; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("at main.ts:"),
        "default build must NOT emit a source location; got:\n{stdout}"
    );
}
