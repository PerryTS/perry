//! Regression test for #5247 (first increment): runtime TypeErrors from the
//! dynamic call-dispatch path ("X is not a function") carry a real source
//! `file:line` in `.stack` / `.message`'s frame when the program is compiled
//! with `--debug-symbols`.
//!
//! Behavior:
//!   • WITH `--debug-symbols`: the thrown TypeError's `.stack` contains
//!     `at <file>:<line>` pointing at the offending call's line.
//!   • WITHOUT the flag (default build): unchanged — `at <anonymous>`.
//!
//! The fixture exercises the highest-value path the increment targets: a
//! method call on an `any`-typed value that is actually a primitive at runtime
//! (`const f: any = 5; f.test();`), which dispatches dynamically and throws
//! `TypeError: (number).test is not a function`.

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

/// `f.test()` lives on line 4 of this fixture (1 = blank from the raw-string
/// leading newline, 2 = `const`, 3 = `try {`, 4 = `f.test();`).
const FIXTURE: &str = r#"
const f: any = 5;
try {
  f.test();
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

#[test]
fn debug_symbols_attaches_file_line_to_not_a_function_throw() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), FIXTURE).expect("write entry");

    let out = compile(root, &["--debug-symbols"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "compile --debug-symbols must succeed; stderr:\n{stderr}"
    );

    let bin = root.join("main_bin");
    let run = Command::new(&bin).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);

    // The dynamic dispatch threw the expected TypeError.
    assert!(
        stdout.contains("MSG:") && stdout.contains("is not a function"),
        "expected a 'is not a function' TypeError; got:\n{stdout}"
    );
    // The stack frame names the source file and the line of `f.test()` (4),
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
fn default_build_keeps_anonymous_frame() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), FIXTURE).expect("write entry");

    let out = compile(root, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "default compile must succeed; stderr:\n{stderr}"
    );

    let bin = root.join("main_bin");
    let run = Command::new(&bin).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);

    assert!(
        stdout.contains("is not a function"),
        "expected a 'is not a function' TypeError; got:\n{stdout}"
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
