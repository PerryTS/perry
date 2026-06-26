//! Regression test for #5437 (Next.js p-queue `PQueue` HTTP-500, dynamic page
//! routes): a class EXPRESSION whose parent is an Ident bound by an in-scope
//! LOCAL must bind `super()` to that LEXICAL local — NOT to an unrelated
//! module-global class that happens to share the (minified) name.
//!
//! Root: in a minified turbopack chunk, dozens of distinct webpack-factory
//! classes are all named with the same single letter (`t`/`u`/`i`). codegen's
//! `super()` resolved the parent through a module-wide `HashMap<name, &Class>`,
//! which keeps only ONE `t`. The bundle's p-queue `class PQueue extends t`
//! (eventemitter3) therefore resolved `t` to superstruct's `StructError` base,
//! and `new PQueue()` (AfterContext's `this.callbackQueue = new (a())()`)
//! inlined StructError's destructuring constructor on the (undefined) options
//! arg → `TypeError: Cannot convert undefined or null to object` → HTTP 500.
//!
//! Fix: when a class EXPRESSION's parent Ident is bound by an in-scope local
//! (`const Base = …; class extends Base {}`), route the parent through the
//! dynamic `extends_expr` path so `super()` invokes the lexically-correct local
//! value at runtime, and have the codegen `Expr::SuperCall` arm prefer that
//! dynamic parent over the same-named module-global class.
//!
//! The discriminating ingredient is the NAME COLLISION between a module-global
//! `class t { … }` (with a destructuring constructor that throws on an
//! undefined arg) and a function-LOCAL `const t = class { … }` that a sibling
//! class expression extends. A bare repro without the collision already worked.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn class_expr_extends_local_not_same_named_module_global_class() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.js");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
"use strict";
// A MODULE-GLOBAL class named `t` whose constructor DESTRUCTURES its argument
// (so constructing it with no/undefined argument throws
// "Cannot convert undefined or null to object"). This stands in for
// superstruct's `StructError` base — the wrong class the bundle's `super()`
// used to resolve to.
class t {
  constructor({ message }) {
    this.message = message;
  }
}
// Keep `t` reachable so it stays in the module's class table.
globalThis.__keepT = t;

// A factory webpack-module: a function scope with a LOCAL `t` (eventemitter3
// stand-in) and a sibling class EXPRESSION `class extends t {}` assigned to an
// exports object. The local `t` SHADOWS the module-global `class t` above.
const mod = (() => {
  // local `t` = the REAL parent (a base class with a no-arg-safe ctor).
  const t = class {
    constructor() {
      this.base = "base-ok";
    }
    ping() { return this.base; }
  };
  const c = {};
  // `class extends t` — must bind to the LOCAL `t`, not the module-global one.
  c.default = class extends t {
    constructor(opts) {
      super();
      // PQueue-style: `opts` may be undefined; Object.assign tolerates it.
      this.q = Object.assign({ tag: "default-tag" }, opts).tag;
    }
    who() { return this.q + "|" + this.ping(); }
  };
  return c;
})();

// Construct via the runtime dynamic-construct path with NO args, exactly like
// AfterContext's `this.callbackQueue = new (a())()`.
function getCls() { return mod.default; }
const inst = new (getCls())();
console.log("r=" + inst.who());
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "r=default-tag|base-ok\n",
        "a class expression `extends t` whose parent Ident is an in-scope local \
         must bind super() to that local — not to a same-named module-global \
         class with a destructuring constructor (#5437 p-queue PQueue HTTP-500)"
    );
}
