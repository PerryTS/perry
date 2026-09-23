//! #11131: a class whose constructor stores `new EventEmitter()` (from a bare
//! CommonJS `require("events")`) in a `#private` field could not read that
//! field back through an inherited method once the class was constructed as
//! the parent of a subclass:
//! `TypeError: Cannot access private member from an object whose class did
//! not declare it`.
//!
//! EventEmitter is incidental. Capturing the CommonJS-wrapper local
//! `EventEmitter` makes both top-level classes per-evaluation classes
//! (`ClassExprFresh`). An instance is stamped with its MOST-DERIVED class
//! evaluation, and the private-brand check compared that stamp against the
//! declaring class exactly, so an ancestor's private member was rejected on
//! every subclass instance. The same defect, without any CommonJS, is #11127
//! (function-local classes) — covered by
//! `test-files/test_gap_11127_private_field_function_local_subclass.ts`.
//!
//! This file pins the literal bare-`require` form, which the gap suite cannot
//! run: Node executes `test-files/*.ts` as ESM (the repo's package.json has
//! `"type": "module"`), where `require` is undefined.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, file_name: &str, source: &str) -> String {
    let entry = dir.join(file_name);
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
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

/// The issue's repro plus a grandchild, a dynamic `new`, a write through an
/// inherited method, and an ergonomic brand check (`#x in o`). Expected output
/// is Node 26.5.1's for the same source saved as `.cjs`.
const SOURCE: &str = r#"
const EventEmitter = require("events");
class Client {
  #socket;
  #count = 0;
  constructor() { this.#socket = new EventEmitter(); }
  kind() { return typeof this.#socket; }
  bump() { return ++this.#count; }
  static has(o) { return #socket in o; }
}
class Sub extends Client {}
class SubSub extends Sub {}
try { console.log("private", new Sub().kind()); } catch (e) { console.log("private threw", e.message); }
console.log("direct", new Client().kind());
console.log("dynamic", new Sub().kind());
const g = new SubSub();
g.bump();
console.log("grandchild", g.kind(), g.bump(), Client.has(g), Client.has({}));
"#;

const EXPECTED: &str = "private object\n\
direct object\n\
dynamic object\n\
grandchild object 2 true false\n";

#[test]
fn cjs_entry_subclass_reads_parent_private_field() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(compile_and_run(dir.path(), "main.cjs", SOURCE), EXPECTED);
}

/// The issue's own spelling: a `.ts` entry whose bare `require` makes Perry
/// wrap it as CommonJS.
#[test]
fn ts_entry_with_bare_require_subclass_reads_parent_private_field() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ts = SOURCE
        .replace("  #socket;", "  #socket: any;")
        .replace("static has(o)", "static has(o: any)")
        .replace("e.message", "(e as Error).message");
    assert_eq!(compile_and_run(dir.path(), "main.ts", &ts), EXPECTED);
}
