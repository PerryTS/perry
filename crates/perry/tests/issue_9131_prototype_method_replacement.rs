//! Regression coverage for #9131: guarded direct method dispatch through a
//! typed-parameter receiver must observe later prototype method replacement
//! and per-instance prototype changes. Neither mutation changes the receiver's
//! class identity, so the fast path must also honor the prototype invalidation
//! state before calling the statically resolved method body.

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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn typed_parameter_calls_observe_prototype_mutations() {
    let stdout = compile_and_run(
        r#"
class Replaced {
  v = 1;
  m() { return 1; }
}
function run(o: Replaced, n: number) {
  let sum = 0;
  for (let i = 0; i < n; i++) sum += (o as any).m();
  return sum;
}

const replaced = new Replaced();
const before = run(replaced, 3);
(Replaced.prototype as any).m = function () { return 100; };
console.log("replace:", before, run(replaced, 3));

class MidLoop {
  m() { return 1; }
}
const mid = new MidLoop();
let midSum = 0;
for (let i = 0; i < 6; i++) {
  midSum += mid.m();
  if (i === 2) (MidLoop.prototype as any).m = function () { return 50; };
}
console.log("mid:", midSum);

class Counter {
  inc() { return 2; }
}
function readCounter(counter: Counter) { return (counter as any).inc(); }
const counter = new Counter();
console.log("counter-before:", readCounter(counter));
Object.setPrototypeOf(counter, { inc: () => 777 });
console.log("counter-after:", readCounter(counter));

class A { m() { return "a"; } }
class B { m() { return "b"; } }
function readA(value: A) { return (value as any).m(); }
const a = new A();
console.log("swap-before:", readA(a));
Object.setPrototypeOf(a, B.prototype);
console.log("swap-after:", readA(a));
"#,
    );

    assert_eq!(
        stdout,
        "replace: 3 300\nmid: 153\ncounter-before: 2\ncounter-after: 777\nswap-before: a\nswap-after: b\n"
    );
}
