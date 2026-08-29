//! Regression for #9087: a declared class used as a value is represented by
//! an INT32-tagged class id, but it is still a Function object for ECMAScript
//! coercion. Dynamic `+` must ToPrimitive the class to its source string
//! instead of adding the numeric class-id payload.

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
fn class_refs_use_string_concatenation_for_dynamic_add() {
    let run = compile_and_run(
        r#"
class K { static m(): number { return 1; } }

const ctor: any = K;
const right = ctor + 1;
const left = 1 + ctor;
console.log(typeof right, right);
console.log(typeof left, left);

let accumulator: any = K;
for (let i = 0; i < 8; i++) accumulator += i;
console.log(typeof accumulator, accumulator);

class Valued { static valueOf(): number { return 40; } }
const valued: any = Valued;
console.log(valued + 2);
"#,
    );
    assert!(
        run.status.success(),
        "compiled program failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "string function K() { [native code] }1\n",
            "string 1function K() { [native code] }\n",
            "string function K() { [native code] }01234567\n",
            "42\n"
        )
    );
}
