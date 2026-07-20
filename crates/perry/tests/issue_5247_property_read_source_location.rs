//! Regression test for #5247 (follow-up to #5250/#5253/#5573): the runtime
//! source-location diagnostics that earlier increments gave the method-dispatch,
//! construct, ReferenceError and bare value-call throws are extended to the
//! **property-read-on-nullish** throw class, gated on `--debug-symbols`:
//!
//!   `obj.prop` where `obj` is `null` / `undefined` →
//!   `TypeError: Cannot read properties of undefined (reading 'prop')`.
//!
//! This is the standalone READ shape (no enclosing call): `const x = o.foo;`
//! and the chained `a.b.c` (inner `a.b` undefined) both lower to a general
//! `Expr::PropertyGet`, which now carries the member access's source
//! `byte_offset` (`member.span.lo.0`). Codegen's generic property-get dispatch
//! (`lower_generic_property_get`) replays it as `js_set_call_location` right
//! before the nullish-receiver throw path (both the inline diamond and the
//! full-outline `js_object_get_field_ic` helper), so `.stack` shows
//! `at <file>:<line>` instead of `<anonymous>`. This is the single most common
//! JS runtime error ("Cannot read properties of undefined") and the last
//! uncovered "property-on-non-object" read case of the bounded #5247 increment.
//!
//! Behavior:
//!   • WITH `--debug-symbols`: the thrown TypeError's `.stack` contains
//!     `at <file>:<line>` pointing at the offending read's line.
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

/// The throwing read lives on line 4 (1 = blank from the raw-string leading
/// newline, 2 = `const`, 3 = `try {`, 4 = `const x = o.foo;`).
const FIXTURE_READ: &str = r#"
const o: any = undefined;
try {
  const x = o.foo;
  console.log(x);
} catch (e: any) {
  console.log("MSG:" + e.message);
  console.log("STACK:" + e.stack);
}
"#;

/// Chained read: `a` is an object, `a.b` is `undefined`, so the throwing access
/// is the outer `.c` on line 4.
const FIXTURE_CHAIN: &str = r#"
const a: any = {};
try {
  const x = a.b.c;
  console.log(x);
} catch (e: any) {
  console.log("MSG:" + e.message);
  console.log("STACK:" + e.stack);
}
"#;

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
        "compile {extra_args:?} must succeed; stderr:\n{stderr}"
    );

    let bin = root.join("main_bin");
    let run = Command::new(&bin).output().expect("run compiled binary");
    // The fixture catches the throw and logs it, so it exits cleanly.
    assert!(
        run.status.success(),
        "compiled binary must exit 0 (throw is caught); status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn debug_symbols_attaches_file_line_to_property_read_on_undefined() {
    let stdout = run_fixture(FIXTURE_READ, &["--debug-symbols"]);
    assert!(
        stdout.contains("MSG:Cannot read properties of undefined"),
        "expected the nullish-read TypeError; got:\n{stdout}"
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
fn debug_symbols_localizes_chained_read_at_the_throwing_access() {
    let stdout = run_fixture(FIXTURE_CHAIN, &["--debug-symbols"]);
    assert!(
        stdout.contains("MSG:Cannot read properties of undefined"),
        "expected the nullish-read TypeError for the chained access; got:\n{stdout}"
    );
    assert!(
        stdout.contains("at main.ts:4"),
        "expected 'at main.ts:4' frame for the chained read; got:\n{stdout}"
    );
}

#[test]
fn default_build_keeps_anonymous_frame_for_property_read() {
    let stdout = run_fixture(FIXTURE_READ, &[]);
    assert!(
        stdout.contains("Cannot read properties of undefined"),
        "expected the nullish-read TypeError; got:\n{stdout}"
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
