//! Regression test for #7845: deeply nested values must raise a catchable
//! exception instead of exhausting the native stack in JSON.stringify or
//! structuredClone.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run_ts(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write");
    let out = dir.path().join("bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&out).output().expect("run");
    assert!(
        run.status.success(),
        "binary exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn deep_stringify_and_clone_errors_are_catchable() {
    let stdout = run_ts(
        r#"
let value: any = 0
for (let i = 0; i < 1002; i++) value = [value]

try {
  JSON.stringify(value)
  console.log("stringify: missed")
} catch (error) {
  console.log("stringify:", error instanceof RangeError)
}
console.log("stringify-after:", JSON.stringify([1, 2]))

try {
  structuredClone(value)
  console.log("clone: missed")
} catch (error) {
  console.log("clone:", error instanceof RangeError)
}
const after: any = structuredClone({ ok: [3, 4] })
console.log("clone-after:", after.ok.join(","))
"#,
    );

    for expected in [
        "stringify: true",
        "stringify-after: [1,2]",
        "clone: true",
        "clone-after: 3,4",
    ] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in:\n{stdout}"
        );
    }
}
