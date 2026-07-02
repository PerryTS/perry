//! Regression: an `Array.prototype` mutator name (`push`/`pop`/`shift`/…)
//! called on a receiver whose *static type* is a (non-class) **named type** —
//! an `interface` or a function/factory return type — must invoke the
//! receiver's OWN method, not the array fast-path intrinsic.
//!
//! Follow-up to #5139, which fixed this for `any`-typed receivers only. HIR
//! lowering's array-only-method fold treated `Type::Named` as a user receiver
//! solely when `lookup_class(name)` found a class, so an interface (not a
//! class) fell through to the `array.push_single` native arm. That reads the
//! plain object's header as an `ArrayHeader`, so the object's own `push`
//! closure never ran and the call was silently dropped — e.g. a server-side
//! framework's `createDocument(): Document` returning `{ push(op) {…} }` had
//! every `doc.push(op)` no-op.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> String {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// `interface Sink { push(x): void }` value: `s.push(...)` must run the object's
/// own `push`, not the array intrinsic (which would read the object header as an
/// ArrayHeader and drop the call).
#[test]
fn interface_typed_object_push_runs_own_method() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
interface Sink {
  items: string[];
  push(x: string): void;
}
const s: Sink = { items: [], push(x: string) { this.items.push(x); } };
s.push("a");
s.push("b");
console.log("len=" + s.items.length);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("len=2"),
        "interface-typed object's own push must run (got stdout: {out:?})"
    );
}

/// A factory typed to return the interface — the receiver's static type is the
/// interface return type. Same fold hazard.
#[test]
fn factory_returned_interface_object_push_runs_own_method() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
interface Sink {
  items: string[];
  push(x: string): void;
}
function makeSink(): Sink {
  const items: string[] = [];
  return { items, push(x: string) { items.push(x); } };
}
const s = makeSink();
s.push("a");
s.push("b");
s.push("c");
console.log("len=" + s.items.length);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("len=3"),
        "factory-returned interface object's own push must run (got stdout: {out:?})"
    );
}

/// Control: genuine arrays must keep their array-builtin semantics (the fix
/// must not regress real `push`/`pop`).
#[test]
fn real_array_mutators_still_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const a: number[] = [];
a.push(1);
a.push(2, 3);
const popped = a.pop();
console.log("sum=" + (a[0] + a[1]) + " len=" + a.length + " popped=" + popped);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("sum=3 len=2 popped=3"),
        "real array push/pop must still work (got stdout: {out:?})"
    );
}
