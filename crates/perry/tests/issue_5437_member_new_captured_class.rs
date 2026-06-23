//! Regression test for #5437 (W6): a member-callee construct
//! `new ns.C()` of a function-nested class that captured an enclosing-scope
//! local DROPPED the captures, so every method that read a captured local
//! saw `undefined`.
//!
//! Root: the bare-identifier `new C()` HIR arm appends the class captures as
//! trailing `LocalGet(cid)` args, but the member-callee form
//! (`new ns.C()`, where `ns` is an object literal whose field was a class
//! expression) is statically routed by codegen's `#740` object-field-alias
//! arm to `lower_new("C", [])` with NO captures appended. The synthesized
//! `__perry_cap_*` ctor params then bound to `undefined`. The
//! `LocalGet(cid)` append cannot be reused at the outer (member) `new`
//! site — the captured enclosing local is out of scope there — so the fix
//! fills the cap params from the class's DECL-SITE capture snapshot
//! (`js_class_capture_value(cid, slot)`, registered at the class declaration
//! by `js_class_register_capture_values`).
//!
//! Exercises (all member-new of a captured class):
//!   - single object capture, no own ctor
//!   - inheritance (sub + base both capturing)
//!   - multiple captures (object + closure + string) + inheritance
//!   - a USER ctor taking args alongside the captures (the user arg must NOT
//!     be mis-split into the capture slot)
//!   - a class declared in a NESTED function capturing two scopes' locals
//!
//! No `node_modules` dependency — the shapes are inline so it runs in CI.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn member_new_of_captured_class_fills_captures_from_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
// Class names are unique per case (distinct classes in one module) so the
// shapes are isolated from one another.

// 1. single object capture, no own ctor, member-new.
function mkMin() {
  const o = { m: "OK" };
  class CMin { r() { return o.m; } }
  return { CMin };
}
const nsMin = mkMin();
console.log("1=" + new nsMin.CMin().r());

// 2. inheritance: sub + base both capture, member-new of the sub.
function mkInherit() {
  const o = { m: "OBJ" };
  class BaseInh { ro() { return o.m; } }
  class SubInh extends BaseInh { rs() { return o.m + "!"; } }
  return { BaseInh, SubInh };
}
const nsInh = mkInherit();
const sInh = new nsInh.SubInh();
console.log("2=" + sInh.ro() + "," + sInh.rs());

// 3. multiple captures (object + closure + string) + inheritance.
function mkMulti() {
  const a = { x: "AX" };
  const b = () => "BCLO";
  const c = "CSTR";
  class BaseMulti { ra() { return a.x; } rc() { return c; } }
  class SubMulti extends BaseMulti { rb() { return b(); } ra2() { return a.x; } }
  return { SubMulti };
}
const nsMulti = mkMulti();
const sMulti = new nsMulti.SubMulti();
console.log("3=" + sMulti.ra() + "," + sMulti.rb() + "," + sMulti.rc() + "," + sMulti.ra2());

// 4. a USER ctor taking args alongside captures — the user arg must bind to
// the user param and the capture must come from the snapshot (the tail-split
// must NOT pull the lone user arg into the capture slot).
function mkUserCtor() {
  const cap = { k: "CAP" };
  class CUser {
    n: string;
    constructor(n: string) { this.n = n; }
    get() { return this.n + "/" + cap.k; }
  }
  return { CUser };
}
const nsUser = mkUserCtor();
console.log("4=" + new nsUser.CUser("ARG").get());

// 5. class declared in a NESTED function, capturing two scopes' locals.
function outer() {
  const tag = "OUTER";
  function inner() {
    const local = { v: "INNER" };
    class CNest { both() { return tag + ":" + local.v; } }
    return { CNest };
  }
  return inner();
}
const nsNest = outer();
console.log("5=" + new nsNest.CNest().both());
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
        stdout, "1=OK\n2=OBJ,OBJ!\n3=AX,BCLO,CSTR,AX\n4=ARG/CAP\n5=OUTER:INNER\n",
        "member-new of a function-nested capturing class must fill the \
         synthesized __perry_cap_* params from the decl-site snapshot (#5437)"
    );
}
