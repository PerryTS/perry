//! Regression coverage for discarded local reads that still carry TDZ checks.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    Command::new(&output).output().expect("run compiled binary")
}

#[test]
fn discarded_forward_captured_local_read_preserves_tdz_throw() {
    let run = compile_and_run(
        r#"
function readValue(): number {
  const capture = () => value;
  value;
  let value = 1;
  return capture();
}
console.log(readValue());
"#,
    );
    assert!(
        !run.status.success(),
        "the TDZ read must terminate execution"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("ReferenceError") || stderr.contains("before initialization"),
        "expected a TDZ ReferenceError, got:\n{stderr}"
    );
}

#[test]
fn discarded_initialized_local_read_remains_nonthrowing() {
    let run = compile_and_run(
        r#"
let value = 1;
value;
console.log(value);
"#,
    );
    assert!(
        run.status.success(),
        "initialized local read failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "1\n");
}
