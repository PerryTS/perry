//! Regression test for #5437 (Next.js Wall #6, p-queue `PQueue` undefined
//! `.default` capture): a class EXPRESSION that EXTENDS a captured local and
//! reads a property off ANOTHER captured local in its constructor — assigned
//! to a member (`c.default = class extends Base { … dep.default … }`) and
//! constructed through the runtime dynamic-construct path (`new (getCls())()`,
//! where the class name is not statically known) — read the captured module
//! ref as `undefined` and threw
//! `TypeError: Cannot read properties of undefined (reading 'default')`.
//!
//! Root: a class EXPRESSION with heritage (or evaluated at module top) takes
//! the shared-template (`ClassRef`) lowering path in `lower_class_expr`, which
//! — unlike the class-DECLARATION path — never emitted a `RegisterClassCaptures`
//! decl-site snapshot. The construct then routes through the runtime
//! `construct_registered_class_ref` → `replay_registered_class_constructor`,
//! which fills the synthesized `__perry_cap_*` ctor params SOLELY from the
//! `CLASS_CAPTURE_VALUES` snapshot. With no snapshot registered, those params
//! arrive `undefined`, so the captured module ref `dep` (and Next.js bundled
//! p-queue's `n.default`) read `undefined`.
//!
//! Fix: emit `RegisterClassCaptures` in the shared-template path of
//! `lower_class_expr` whenever the class expression captures enclosing-scope
//! locals, mirroring the class-declaration path. The snapshot then exists and
//! `replay_registered_class_constructor` fills the cap params correctly for
//! every construction path, including the runtime dynamic one.
//!
//! Mirrors the minimal repro that pinned the wall: the interop default-getter
//! (`() => mod.default`) returning the class + construction from inside another
//! class's constructor via `new (tH())()` are the discriminating ingredients
//! (a bare `new (mod.default)()` at top level already worked).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn class_expr_with_heritage_capture_via_dynamic_construct_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.js");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
"use strict";
// Faithful to Next.js's bundled p-queue: a nested IIFE "webpack module" with
// module-scope `let` bindings assigned from an inner require, then a class
// EXPRESSION assigned to `c.default` that EXTENDS a captured `Base` and reads
// `.default` off a captured `dep` in its constructor.
const pqMod = (() => {
  let Base, dep;
  var inner = {
    993: (m) => { m.exports = class { ping() { return "base"; } }; },
    821: (m) => { m.exports = { default: "DEP_DEFAULT" }; },
  };
  const cache = {};
  function req(id) {
    if (cache[id]) return cache[id].exports;
    const m = (cache[id] = { exports: {} });
    inner[id](m);
    return m.exports;
  }
  Base = req(993);
  dep = req(821);
  const c = {};
  Object.defineProperty(c, "__esModule", { value: true });
  c.default = class extends Base {
    constructor(opts) {
      super();
      this.q = Object.assign({ queueClass: dep.default }, opts).queueClass;
    }
    who() { return this.q; }
  };
  return c;
})();

// webpack interop default-getter (`__webpack_require__.n`): returns a function
// that yields module.default.
function interopDefault(mod) {
  return mod && mod.__esModule ? () => mod.default : () => mod;
}
const tH = interopDefault(pqMod);

// Another class that constructs the captured class from INSIDE its ctor, via
// the runtime dynamic-construct path `new (tH())()` (class name not static).
class Consumer {
  constructor() {
    this.cb = new (tH())();
  }
  result() {
    return this.cb.who() + "|" + this.cb.ping();
  }
}
const inst = new Consumer();
console.log("r=" + inst.result());
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
        stdout, "r=DEP_DEFAULT|base\n",
        "a class-expression with heritage that captures enclosing locals and is \
         constructed via the runtime dynamic path must resolve its captures from \
         the decl-site snapshot (not undefined) — #5437 p-queue PQueue Wall #6"
    );
}
