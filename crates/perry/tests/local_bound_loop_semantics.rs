use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &Path, source: &str) -> String {
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

    let run = Command::new(&output).output().expect("run compiled binary");
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
fn local_loop_bounds_match_js_trip_counts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function mutatedBound(): number {
  let n = 3;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
    n = 0;
  }
  return count;
}

function fractionalBound(): number {
  let n = 1.5;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
  }
  return count;
}

function nanBound(): number {
  let n = 0 / 0;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
  }
  return count;
}

function infiniteMutatedBound(): number {
  let n = 1 / 0;
  let count = 0;
  for (let i = 0; i < n; i++) {
    count = count + 1;
    n = 0;
  }
  return count;
}

console.log(mutatedBound());
console.log(fractionalBound());
console.log(nanBound());
console.log(infiniteMutatedBound());
"#,
    );
    assert_eq!(stdout, "1\n2\n0\n1\n");
}
