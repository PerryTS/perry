//! Regression test for #5127: an `Error` subclass that forwards options via
//! `super(message, options)` dropped the ES2022 `cause` — `this.cause` was
//! `undefined` afterward, even though a plain `new Error(msg, { cause })`
//! (no subclass) worked.
//!
//! Root cause: the Error-like `super(...)` codegen arm only assigned
//! `this.message = args[0]` and `this.name = <parent>`; it ignored `args[1]`
//! (the options object). Fix: when a second arg is present, call
//! `js_error_apply_cause_to_object`, which installs a non-enumerable own
//! `cause` property on the subclass instance from `options.cause` (matching
//! Node's `InstallErrorCause`).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn error_subclass_forwards_cause_through_super() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class AppError extends Error {
  constructor(msg: string, opts?: ErrorOptions) { super(msg, opts); this.name = "AppError"; }
}
const e = new AppError("high-level", { cause: new TypeError("low-level") });
console.log(e.message, (e.cause as Error)?.message);
console.log(e.name, e instanceof Error);
// `cause` is a non-enumerable own property (Node semantics).
console.log(Object.keys(e).includes("cause"), Object.prototype.hasOwnProperty.call(e, "cause"));

// Plain (non-subclass) Error with cause still works.
const p = new Error("m", { cause: 42 });
console.log(p.cause);

// No options forwarded => no cause.
class B extends Error { constructor(m: string){ super(m); } }
console.log(new B("x").cause);
"#,
    );
    assert_eq!(
        stdout,
        "high-level low-level\nAppError true\nfalse true\n42\nundefined\n"
    );
}
