//! Regression coverage for eliding full Arguments-object materialization when
//! a function observes only `arguments.length`.
//!
//! Call lowering already supplies a marked raw-argument Array containing every
//! actual argument. Its length is therefore exact for ordinary functions,
//! methods, function expressions, and captured outer `arguments`. Any other
//! observation must retain the full ECMAScript Arguments object.

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
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
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
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn length_only_works_across_callable_kinds_and_arities() {
    let stdout = compile_and_run(
        r#"
function declared(a?: any, b?: any) { return arguments.length; }
const expression = function (a?: any) { return arguments.length; };
class Probe {
  method(a?: any) { return arguments.length; }
  static method(a?: any) { return arguments.length; }
}
function captured(a?: any) {
  return () => arguments.length;
}

const probe = new Probe();
console.log(declared(), declared(1), declared(1, 2, 3));
console.log(expression(), expression(1, 2));
console.log(probe.method(), probe.method(1, 2));
console.log(Probe.method(), Probe.method(1, 2, 3));
console.log(captured(1, 2, 3, 4)());
"#,
    );
    assert_eq!(stdout, "0 1 3\n0 2\n0 2\n0 3\n4\n");
}

#[test]
fn observable_and_mixed_arguments_uses_still_materialize() {
    let stdout = compile_and_run(
        r#"
import { types } from "node:util";

function observable(a?: any) {
  return types.isArgumentsObject(arguments);
}
function mixed(a?: any) {
  return arguments.length + ":" + arguments[0] + ":" +
    types.isArgumentsObject(arguments);
}
function writesLength() {
  arguments.length = 7;
  return arguments.length;
}

console.log(observable(1));
console.log(mixed("first", "second"));
console.log(writesLength(1, 2));
"#,
    );
    assert_eq!(stdout, "true\n2:first:true\n7\n");
}
