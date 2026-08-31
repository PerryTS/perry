//! #9252: tape-backed arrays must expose JSON numbers as their `f64` values,
//! even when stringify can otherwise reuse the retained source bytes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
const inputs = [
  "[1e308]",
  "[-1e308]",
  "[1e-400]",
  "[5e-324]",
  "[1.7976931348623157e308]",
  "[9007199254740993]",
  "[1.25e2]",
  "[1e400]",
  "[-1e-400]",
  "[-0]",
  "[1.0]",
  "[1e308,1,1e-400,2,9007199254740993,5e-324]",
  "[{\"n\":1e308,\"text\":\"1e-400\"}]"
];
for (const input of inputs) {
  console.log(JSON.stringify(JSON.parse(input)));
}
"#;

const EXPECTED: &str = "[1e+308]\n\
[-1e+308]\n\
[0]\n\
[5e-324]\n\
[1.7976931348623157e+308]\n\
[9007199254740992]\n\
[125]\n\
[null]\n\
[0]\n\
[0]\n\
[1]\n\
[1e+308,1,0,2,9007199254740992,5e-324]\n\
[{\"n\":1e+308,\"text\":\"1e-400\"}]\n";

fn compile(dir: &Path) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-auto-optimize")
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run(bin: &Path, dir: &Path, tape_mode: &str) -> Output {
    Command::new(bin)
        .current_dir(dir)
        .env("PERRY_JSON_TAPE", tape_mode)
        .output()
        .expect("run compiled binary")
}

#[test]
fn json_tape_numbers_round_trip_through_f64() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(dir.path());
    for tape_mode in ["0", "1"] {
        let output = run(&bin, dir.path(), tape_mode);
        assert!(
            output.status.success(),
            "binary failed with tape={tape_mode}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            EXPECTED,
            "tape={tape_mode}"
        );
    }
}
