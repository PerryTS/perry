//! Regression test for #5925: an await inside a comma-sequence was hoisted
//! above the containing statement, so the awaited operand evaluated BEFORE
//! the sequence's earlier operands ran. The minified shape
//! `this.l = new L, this.p = await this.l.start()` then read `this.l`'s
//! pre-assignment value (`null`) and threw
//! "Cannot read properties of null (reading 'start')" — while node prints
//! the awaited result. Minifiers comma-fold consecutive statements, so any
//! `const x = new X(); const y = await x.m();` pair in bundled async code
//! hits this.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
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

    let run = Command::new(&output)
        .current_dir(dir)
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

/// The #5925 shape: assignment comma-chained with an await whose receiver is
/// the just-assigned field. Pre-fix: "ERR TypeError: Cannot read properties
/// of null (reading 'start')".
#[test]
fn seq_assignment_runs_before_awaited_receiver_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Listener {
  async start() {
    return 42;
  }
}

class Flow {
  authCodeListener: any = null;
  port: any = null;

  async go() {
    this.authCodeListener = new Listener, this.port = await this.authCodeListener.start();
    return this.port;
  }
}

const f = new Flow();
f.go()
  .then((v) => console.log("GOT", v))
  .catch((e) => console.log("ERR", String(e)));
"#,
    );
    assert!(
        stdout.contains("GOT 42"),
        "sequence operand must run before the awaited receiver is read\nstdout:\n{stdout}"
    );
}

/// Sequences in let-init and return position, and awaits in an EARLIER
/// operand: all operands must still run in evaluation order.
#[test]
fn seq_await_order_let_return_and_early_operand() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const order: string[] = [];

async function tag(name: string, v: number) {
  order.push(name);
  return v;
}

let sink = 0;

async function letInit() {
  // let-init position: seq's first operand must run before the awaited call
  let x = (sink = 7, await tag("after-sink", sink + 1));
  return x;
}

async function returnPos() {
  // return position, await in the EARLIER operand too
  return (await tag("first", 1), sink = 9, await tag("second", sink + 1));
}

async function main() {
  const a = await letInit();
  console.log("LET", a, "sink", sink);
  const b = await returnPos();
  console.log("RET", b, "sink", sink);
  console.log("ORDER", order.join(","));
}

main().catch((e) => console.log("ERR", String(e)));
"#,
    );
    assert!(
        stdout.contains("LET 8 sink 7"),
        "let-init sequence must assign before the await\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("RET 10 sink 9"),
        "return-position sequence must evaluate operands in order\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("ORDER after-sink,first,second"),
        "awaits must run in source evaluation order\nstdout:\n{stdout}"
    );
}
