//! Regression test for #4972: a derived class with NO user-written
//! constructor whose method bodies capture an enclosing-scope local threw
//! "Must call super constructor in derived class before accessing 'this'"
//! at construction time (`test-http-client-readable`).
//!
//! Root cause: `synthesize_class_captures` (#212/#740) materializes a
//! capture-stashing constructor (`__perry_cap_<id>` params + `this`
//! PropertySets) when class members reference outer locals. That synthesized
//! ctor contained no `super()` call, so codegen's static derived-ctor TDZ
//! check in `lower_call/new.rs` (own ctor + heritage + no `super()` ⇒
//! unconditional ReferenceError) fired for a class the user never wrote a
//! ctor for. The canonical shape from the node corpus:
//!
//! ```ts
//! const Duplex = require('stream').Duplex;
//! class FakeAgent extends http.Agent {
//!   createConnection() { return new Duplex(); }   // captures `Duplex`
//! }
//! new FakeAgent();                                 // threw pre-fix
//! ```
//!
//! Fix: when the fresh (non-user) ctor is synthesized for a class with
//! heritage, seed its body with `super()` — mirroring the spec default ctor
//! `constructor(...args) { super(...args) }`. This both suppresses the
//! bogus static throw and routes known user-class parents through the
//! inline-parent-ctor arm so the parent body actually runs.

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
        "compiled binary failed (pre-fix: 'Must call super constructor' \
         ReferenceError from the synthesized capture ctor)\nstatus: {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The corpus shape (`test-http-client-readable`): a no-ctor subclass of a
/// native member base whose method captures a module-level binding
/// (`Duplex` via the stream-class destructure alias). Pre-fix `new
/// FakeAgent()` threw the derived-ctor TDZ ReferenceError.
#[test]
fn no_ctor_subclass_of_native_member_base_with_capture_constructs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const http = require('http');
const Duplex = require('stream').Duplex;

class FakeAgent extends http.Agent {
  createConnection() {
    const s = new Duplex();
    return s;
  }
}

const a = new FakeAgent();
console.log('ctor-ok', typeof a === 'object');
const s = a.createConnection();
console.log('capture-ok', typeof s === 'object');
"#,
    );
    assert_eq!(
        stdout, "ctor-ok true\ncapture-ok true\n",
        "no-ctor subclass with captured stream alias must construct"
    );
}

/// The minimal non-http repro the issue asks for: a function-nested derived
/// class whose method captures an enclosing-fn local. The synthesized
/// capture ctor must call `super()` so (a) construction doesn't throw and
/// (b) the user parent's ctor body actually runs (`this.p = 1`).
#[test]
fn nested_derived_class_with_capture_runs_parent_ctor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function make() {
  const v = 42;
  class P {
    p: number;
    constructor() {
      this.p = 1;
    }
  }
  class C extends P {
    m() {
      return v;
    }
  }
  return new C();
}
const c = make();
console.log('parent-ctor', c.p);
console.log('capture', c.m());
"#,
    );
    assert_eq!(
        stdout, "parent-ctor 1\ncapture 42\n",
        "parent ctor must run via the synthesized super() and the capture must resolve"
    );
}

/// Guard: a non-derived class with captures keeps its plain synthesized ctor
/// (no super() seeded — `Expr::SuperCall` outside a heritage context is a
/// soft no-op, but the base-class path shouldn't change at all).
#[test]
fn base_class_with_capture_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function make() {
  const tag = 'hello';
  class Plain {
    m() {
      return tag;
    }
  }
  return new Plain();
}
console.log('base', make().m());
"#,
    );
    assert_eq!(
        stdout, "base hello\n",
        "base-class capture path must be unchanged"
    );
}
