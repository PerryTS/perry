//! Regression tests for the two CodeRabbit correctness findings on the #5437
//! cross-module member-`new` capture fix (the synthesized-constructor capture
//! rebind in `synthesize_class_captures`).
//!
//! FINDING 1 (param-first): the original rebind used
//! `js_class_capture_value_or` (SNAPSHOT-first) — the decl-site snapshot won
//! whenever it held a real value, even over the LIVE `new`-site cap arg. That
//! is wrong for a SAME-module `new` of a class whose captured outer was MUTATED
//! after the class declaration: the stale decl-site snapshot overrode the live
//! mutated value. The rebind is now PARAM-first
//! (`js_param_or_class_capture_value`): the live param wins whenever present;
//! the decl-site snapshot is consulted ONLY when the param is `undefined` (the
//! cross-module construct path, which drops the cap arg).
//!
//! FINDING 2 (rebind placement): the param rebinds were inserted AFTER
//! `super()`, so a derived ctor that reads a captured outer BEFORE calling
//! `super()` ran before the snapshot recovery. The rebinds now go at FUNCTION
//! ENTRY (before any pre-`super()` user code); only the `this.__perry_cap_*`
//! field stashes wait until after `super()`.
//!
//! Both expected outputs are byte-for-byte what `node
//! --experimental-strip-types` prints.

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
        .current_dir(dir)
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

/// FINDING 1 guard: a same-module `new C()` whose captured outer (`x`) was
/// MUTATED after the class declaration must read the LIVE value `"b"`, not the
/// stale decl-site snapshot `"a"`. Pre-rework (snapshot-first) this printed
/// `"a"`. node `--experimental-strip-types` prints `"b"`.
#[test]
fn same_module_mutated_capture_uses_live_param_not_stale_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let x = "a";
class C {
  x: any;
  constructor() {
    this.x = x;
  }
}
x = "b";
const c = new C();
console.log(c.x);
"#,
    );
    assert_eq!(
        stdout, "b\n",
        "same-module `new` must use the LIVE mutated capture (param-first), \
         not the stale decl-site snapshot"
    );
}

/// FINDING 2 guard: a DERIVED ctor reads a captured outer (`cap`) BEFORE
/// calling `super()`. The capture rebind must already be in effect at that
/// point (rebinds at function entry), so `cap` resolves to its live value.
/// Pre-rework the rebinds sat after `super()`, so the pre-`super()` read saw
/// the un-recovered param. node prints `base` / `live-cap`.
#[test]
fn derived_ctor_reads_capture_before_super() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function make() {
  const cap = "live-cap";
  class B {
    b: any;
    constructor() {
      this.b = "base";
    }
  }
  class D extends B {
    v: any;
    constructor() {
      const v = cap; // read captured outer BEFORE super()
      super();
      this.v = v;
    }
  }
  return new D();
}
const d = make();
console.log(d.b);
console.log(d.v);
"#,
    );
    assert_eq!(
        stdout, "base\nlive-cap\n",
        "derived ctor reading a captured outer before super() must see the \
         recovered value (rebinds at function entry)"
    );
}
