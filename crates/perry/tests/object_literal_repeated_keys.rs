//! An object literal that repeats a key has one own property for it, at the
//! first occurrence's position, holding the last definition. The literal
//! cases are `test-files/test_gap_object_literal_repeated_keys.ts` (checked
//! against node); this file holds what node cannot run: enums, whose
//! reverse map repeats a key when two members share a value. Expected output
//! is TypeScript 5.8 (`tsc --target ES2022`) run by node.

use std::path::PathBuf;
use std::process::Command;

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let perry = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime_dir = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| perry.parent().expect("binary directory").to_path_buf());
    let compile = Command::new(&perry)
        .current_dir(dir.path())
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir)
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "run failed ({:?})\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("UTF-8 stdout")
}

/// `P[0]` is the last member with value 0, and `"0"` is one key.
#[test]
fn enum_members_sharing_a_value_have_one_reverse_key() {
    let source = r#"
enum P { A = 0, B = 0, C = 1, D = 1, E }
console.log(Object.keys(P).join(","), P[0], P[1], P[2], P.B);
const index = (e: any, i: number) => e[i];
console.log(index(P, 0), index(P, 1), Object.values(P).length);
function local() {
  enum Q { Lowest = 0, Comma = 0, Spread = 0, Yield = 1 }
  return Object.keys(Q).join(",") + " " + Q[0];
}
console.log(local());
enum S { X = "a", Y = "a" }
console.log(Object.keys(S).join(","), S.X, S.Y);
"#;
    assert_eq!(
        compile_and_run(source),
        "0,1,2,A,B,C,D,E B D E 0\nB D 8\n0,1,Lowest,Comma,Spread,Yield Spread\nX,Y a a\n"
    );
}
