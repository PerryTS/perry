//! Regression test for #5247 (follow-up to #5250/#5253): the runtime
//! source-location diagnostics that #5250/#5253 gave the method-dispatch,
//! construct and ReferenceError throws are extended to the **bare value-call**
//! throw class, gated on `--debug-symbols`:
//!
//!   `f()` where `f` is not callable → `TypeError: value is not a function`.
//!   `const f: any = 5; f();` lowers to `Expr::Call` with a `LocalGet` callee
//!   that no static dispatch claims, so codegen's closure-call fallthrough
//!   (`try_lower_closure_call_fallthrough`) emits `js_closure_unbox_callee_checked`,
//!   which throws via `throw_not_callable` → `make_stack`. This is the shape
//!   that localizes nanoid/yup's `value is not a function`.
//!
//! Behavior:
//!   • WITH `--debug-symbols`: the thrown TypeError's `.stack` contains
//!     `at <file>:<line>` pointing at the offending call's line.
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
    // The fixtures catch the throw and `console.log` it, so the program exits
    // cleanly. Assert that, so a crash / non-zero exit can't masquerade as a
    // plain assertion failure on partial stdout.
    assert!(
        run.status.success(),
        "compiled binary must exit successfully; stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// `f()` is on line 4 (1 = blank from the raw-string leading newline,
/// 2 = `const`, 3 = `try {`, 4 = `f();`).
const VALUE_CALL_FIXTURE: &str = r#"
const f: any = 5;
try {
  f();
} catch (e: any) {
  console.log("MSG:" + e.message);
  console.log("STACK:" + e.stack);
}
"#;

#[test]
fn debug_symbols_attaches_file_line_to_value_call_throw() {
    let stdout = run_fixture(VALUE_CALL_FIXTURE, &["--debug-symbols"]);
    // The non-callable value-call threw the expected TypeError.
    assert!(
        stdout.contains("MSG:") && stdout.contains("value is not a function"),
        "expected a 'value is not a function' TypeError; got:\n{stdout}"
    );
    // The stack frame names the source file and the line of `f()` (4),
    // not `<anonymous>`.
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
fn default_build_keeps_anonymous_frame_for_value_call() {
    let stdout = run_fixture(VALUE_CALL_FIXTURE, &[]);
    assert!(
        stdout.contains("value is not a function"),
        "expected a 'value is not a function' TypeError; got:\n{stdout}"
    );
    // Default build is unchanged: the coarse <anonymous> frame, no file:line.
    assert!(
        stdout.contains("at <anonymous>"),
        "default build must keep the <anonymous> frame; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("at main.ts:"),
        "default build must NOT emit a source location; got:\n{stdout}"
    );
}
