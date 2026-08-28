//! Regression test: a derived class declared inside a function (so it captures
//! enclosing locals) whose `super()` call is not its own statement.
//!
//! `synthesize_class_captures` stashes every captured outer onto the instance
//! (`this.__perry_cap_<id> = param`) right after `super()` so methods called
//! from the constructor can read it. It located `super()` only as a top-level
//! `Stmt::Expr(SuperCall)`; the minifier's `super(a), this.x = b, …` comma
//! sequence (Next's `AppRouteRouteModule`) and p-queue's `if (super(), …)`
//! were not found, and the stash went to constructor ENTRY — before `super()`.
//! That was a silent write onto the pre-allocated receiver until #8630 added
//! the spec derived-`this` TDZ check, after which every construction threw
//! `ReferenceError: Must call super constructor in derived class before
//! accessing 'this' or returning from derived constructor`. Coop's Next.js
//! App Route fixture died at module init on `new AppRouteRouteModule({…})`.
//!
//! Fix: place the early stash after the statement that completes `super()`,
//! splitting a leading-`super()` comma sequence so the stash sits between the
//! call and the rest of the sequence.

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
         ReferenceError from a capture stash placed before super())\nstatus: {:?}\n\
         stdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The Next.js shape: the class lives in a module-function scope, captures
/// two of its locals, writes its fields in one comma sequence after
/// `super(…)`, and is constructed through the runtime path (`new ns.Class`)
/// that supplies no capture args — exactly `new w.AppRouteRouteModule({…})`.
#[test]
fn comma_sequence_super_with_captures_constructs_via_runtime_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const mod: any = {};
(function (exports: any) {
  const shared = { tag: "outer" };
  const helper = (x: any) => x + 1;
  class Base {
    constructor(opts: any) { (this as any).definition = opts.definition; }
  }
  class Derived extends Base {
    constructor({ definition: r, name: n }: any) {
      super({ definition: r }), (this as any).name = n, (this as any).tag = shared.tag, (this as any).h = helper(1);
    }
    describe() { return `${shared.tag}/${helper(2)}`; }
  }
  exports.Derived = Derived;
})(mod);
const inst = new mod.Derived({ definition: "d", name: "x" });
console.log(inst.definition, inst.name, inst.tag, inst.h, inst.describe());
"#,
    );
    assert_eq!(stdout, "d x outer 2 outer/3\n");
}

/// p-queue's shape: `super()` as the first operand of an `if` test.
#[test]
fn if_test_super_with_captures_constructs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const shared = { tag: "outer" };
function make(e: any) {
  class Base { constructor() { (this as any).base = 1; } }
  class Derived extends Base {
    constructor(e: any) {
      var q;
      if (super(), (this as any).count = 0, (this as any).tag = shared.tag, !e) { q = 1; }
      (this as any).q = q;
    }
    readTag() { return shared.tag; }
  }
  return new Derived(e);
}
const a = make(undefined) as any;
const b = make(5) as any;
console.log(a.base, a.count, a.tag, a.q, a.readTag(), b.q);
"#,
    );
    assert_eq!(stdout, "1 0 outer 1 outer undefined\n");
}

/// Guard: the plain `super();` statement shape keeps the early stash right
/// after the call, so a method invoked from the constructor still resolves a
/// captured outer through its `this.__perry_cap_*` field (#5437).
#[test]
fn statement_super_with_captures_still_stashes_early() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const mod: any = {};
(function (exports: any) {
  const shared = { tag: "outer" };
  class Base { constructor() { (this as any).base = 1; } }
  class Derived extends Base {
    constructor() {
      super();
      (this as any).seen = this.read();
    }
    read() { return shared.tag; }
  }
  exports.Derived = Derived;
})(mod);
const inst = new mod.Derived();
console.log(inst.base, inst.seen);
"#,
    );
    assert_eq!(stdout, "1 outer\n");
}
