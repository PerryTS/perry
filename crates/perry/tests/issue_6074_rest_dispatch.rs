//! Regression for #6074: rest-param closures with 8+ fixed params must
//! dispatch to the compiled body instead of silently returning `undefined`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("--no-cache")
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
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
fn rest_param_dispatch_with_eight_fixed_args_matches_node() {
    let stdout = compile_and_run(
        r#"
const f = (
  a1: number,
  _a2: number,
  _a3: number,
  _a4: number,
  _a5: number,
  _a6: number,
  _a7: number,
  a8: number,
  ...rest: number[]
) => a1 + a8 + rest.length;

const direct: any = f;
const obj = { f };
console.log("direct", direct(1, 2, 3, 4, 5, 6, 7, 8, 9, 10));
console.log("method", obj.f(1, 2, 3, 4, 5, 6, 7, 8, 9, 10));
"#,
    );
    assert_eq!(stdout, "direct 11\nmethod 11\n");
}

#[test]
fn rest_param_dispatch_with_fifteen_fixed_args_matches_node() {
    let stdout = compile_and_run(
        r#"
const f = (
  a1: number,
  _a2: number,
  _a3: number,
  _a4: number,
  _a5: number,
  _a6: number,
  _a7: number,
  a8: number,
  _a9: number,
  _a10: number,
  _a11: number,
  _a12: number,
  _a13: number,
  _a14: number,
  a15: number,
  ...rest: number[]
) => a1 + a8 + a15 + rest.length;

const direct: any = f;
const obj = { f };
console.log("direct15", direct(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17));
console.log("method15", obj.f(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17));
"#,
    );
    assert_eq!(stdout, "direct15 26\nmethod15 26\n");
}
