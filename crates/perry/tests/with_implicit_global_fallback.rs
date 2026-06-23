//! Regression test (#5579, `with`-statement cluster): a bare unqualified read
//! of a name AFTER a `with` block, where the `with`-set fallback never fired,
//! must resolve against the global object — not unconditionally throw.
//!
//! `with (o) { p1 = 'x1'; }` lowers the assignment to a `WithSet` whose
//! fallback is a sloppy-implicit global that starts as a HOLE sentinel. When
//! `o` OWNS `p1`, the write goes to `o` and the sentinel is never replaced. A
//! later bare `p1` routes through `js_with_implicit_read`, which used to throw
//! `ReferenceError` whenever the slot was still the HOLE — even when `p1` was a
//! real own property of `globalThis` set independently of the `with`. Per a
//! conforming host (and Node's `vm.runInThisContext` Test262 oracle) the read
//! must observe the global value (`globalThis.p1`). This is the with-statement
//! analogue of the #5579 global-scope regression
//! (`language/statements/with/S12.10_A1.*`).
//!
//! The fix makes the HOLE path defer to `js_global_get_or_throw_unresolved`,
//! so a present global property reads back and a genuinely unresolvable name
//! (`with/12.10-0-7`: `var o = {foo:1}; with(o){foo=42} foo`) still throws.

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

#[test]
fn with_implicit_read_falls_back_to_global() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
// Case 1: the with-env object OWNS the name, so `p1 = 'x1'` writes the object
// and the sloppy-implicit fallback never fires. The bare `p1` read after the
// `with` must still see the independently-set global property (=== 1), not
// throw a ReferenceError off the unfilled HOLE sentinel.
(globalThis as any).p1 = 1;
const o1: any = { p1: "a" };
with (o1) {
  p1 = "x1";
}
if (p1 !== 1) throw new Error("case1: expected p1===1, got " + p1);
if (o1.p1 !== "x1") throw new Error("case1b: expected o1.p1==='x1', got " + o1.p1);
console.log("case1-ok");

// Case 2: the global genuinely lacks the name, so the bare read after the
// `with` must still throw ReferenceError (test262 with/12.10-0-7 preserved).
const o2: any = { foo: 1 };
with (o2) {
  foo = 42;
}
let threw = false;
try {
  foo;
} catch (e) {
  threw = e instanceof ReferenceError;
}
if (!threw) throw new Error("case2: expected ReferenceError for unresolved `foo`");
console.log("case2-ok");

// Case 3: the `with` is nested INSIDE a function, so the sloppy-implicit
// slot is only declared at module scope by the hoist (the in-body HOLE init
// lands in the callee's own frame). The hoist must seed that slot with the
// HOLE sentinel — not `undefined` — so the bare module-scope read still falls
// back to the global (S12.10_A1.2/A1.3/A3.2).
(globalThis as any).q1 = 7;
const o3: any = { q1: "a" };
function g() {
  with (o3) {
    q1 = "z1";
  }
}
g();
if (q1 !== 7) throw new Error("case3: expected q1===7, got " + q1);
if (o3.q1 !== "z1") throw new Error("case3b: expected o3.q1==='z1', got " + o3.q1);
console.log("case3-ok");

// Case 4: function-nested `with` whose env LACKS the name, so the assignment
// must CREATE the binding (not write a throwaway shadow). The module-scope
// slot is the one the fallback writes and the later read observes — exercises
// that the with-set fallback inside a callee targets the module slot, not a
// function-local shadow (S12.10_A1.2/A1.3 `p5`).
const o4: any = {}; // no `r1`
function h() {
  with (o4) {
    r1 = "made-global";
  }
}
h();
if (r1 !== "made-global") throw new Error("case4: expected r1==='made-global', got " + r1);
console.log("case4-ok");

// Case 5: a global property explicitly set to `undefined` is PRESENT, so the
// post-`with` read must observe `undefined` — not mis-throw a ReferenceError
// off the unfilled HOLE. The fallback resolves by existence, not value.
(globalThis as any).u1 = undefined;
const o5: any = { u1: 0 };
with (o5) {
  u1 = 1;
}
let u1WasUndefined = false;
try {
  u1WasUndefined = u1 === undefined;
} catch (e) {
  throw new Error("case5: present-but-undefined global must not throw");
}
if (!u1WasUndefined) throw new Error("case5: expected u1===undefined");
if (o5.u1 !== 1) throw new Error("case5b: expected o5.u1===1, got " + o5.u1);
console.log("case5-ok");

console.log("ok");
"#,
    );
    assert_eq!(
        stdout, "case1-ok\ncase2-ok\ncase3-ok\ncase4-ok\ncase5-ok\nok\n",
        "with-implicit read must fall back to globalThis, still throwing for truly-unresolved names"
    );
}
