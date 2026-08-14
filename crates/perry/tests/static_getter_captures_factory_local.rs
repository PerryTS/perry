//! Regression: a static accessor on a class expression returned by a factory
//! must read that evaluation's captured locals.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn static_getter_reads_factory_local_capture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.mjs");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
function factory(value) {
  const read = () => value;
  return class {
    static get captured() {
      return read();
    }
  };
}

const First = factory(41);
const Second = factory(42);
console.log(First.captured, Second.captured);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "41 42");
}
