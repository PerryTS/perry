//! `sub.fill(value, start?, end?)` on a `class X extends Array` instance.
//!
//! `js_array_subclass_init` installs `fill` on the instance (node inherits it
//! from `Array.prototype`; perry has no such prototype object for these), and
//! that stub had arity 1 — so every `start`/`end` argument was dropped and the
//! whole array was overwritten.
use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.js");
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

/// Every `fill` form node accepts, on a subclass instance: value only, a start,
/// a start and end, and a negative start.
#[test]
fn array_subclass_fill_honours_start_and_end() {
    let run = compile_and_run(
        r#"
class A extends Array {}
const out = [];
const a = new A(); a.push(1, 2, 3, 4); out.push("all=" + a.fill(9).join("|"));
const b = new A(); b.push(1, 2, 3, 4); out.push("from1=" + b.fill(9, 1).join("|"));
const c = new A(); c.push(1, 2, 3, 4); out.push("1to3=" + c.fill(9, 1, 3).join("|"));
const d = new A(); d.push(1, 2, 3, 4); out.push("neg=" + d.fill(9, -2).join("|"));
console.log(out.join(" "));
"#,
    );
    assert!(
        run.status.success(),
        "the program must exit cleanly\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "all=9|9|9|9 from1=1|9|9|9 1to3=1|9|9|4 neg=1|2|9|9\n"
    );
}
