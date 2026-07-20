//! Regression tests for #6658 (pi wall #7): an extracted builtin method used
//! as a callback with an explicit `thisArg` — `arr.forEach(set.add, set)` —
//! must invoke the builtin with `this = thisArg`, exactly as node does.
//!
//! Trigger in the wild: @babel/types' alias-expansion loop
//! (`e5 ? e5.forEach(t4.add, t4) : t4.add(r4)`, pi-bundle.mjs:203827) threw
//! "TypeError: Method Set.prototype.add called on incompatible receiver"
//! during pi-native module init.
//!
//! Root cause: the DYNAMIC method-dispatch tower (`js_native_call_method` →
//! the dense-array arms in `native_call_method/handle_methods.rs`) dropped
//! `args[1]` for the whole Array.prototype callback family and dispatched to
//! the dense helpers, which bind the callback's `this` to undefined (the spec
//! rule for an ABSENT thisArg). The STATIC lowering already routed
//! explicit-thisArg calls through the this-binding `js_arraylike_*` engine —
//! a receiver only reaches the dynamic tower when codegen can't prove its
//! type (here: an object member read through a DYNAMIC key), which is why
//! only the combined @babel/types shape reproduced and every
//! statically-provable simplification worked. The fix mirrors the static
//! rule in the dynamic tower: with a thisArg present, route through
//! `dispatch_arraylike_read_method`.
//!
//! Also pinned: node/V8's brand-check TypeError message, receiver rendering
//! included (`NoSideEffectsToString`: "#<Object>", "undefined", "5.5", ...).
//!
//! Expected outputs are node v26's, byte for byte.

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

/// The issue's combined shape verbatim, the five previously-ruled-out simpler
/// shapes as anchors, and the minimal dynamic-tower trigger (a dynamic-key
/// member read defeats flow typing, so `.forEach` dispatches through the
/// runtime tower that dropped the thisArg) with Set.prototype.add,
/// Map.prototype.set, and Array.prototype.push extracted as callbacks.
#[test]
fn extracted_builtin_method_callback_thisarg_matches_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const FLIPPED: any = { A: ["x", "y"], B: null };
const allExpandedTypes: any[] = [
  { types: ["A", "B"], set: new Set() },
  { types: ["B"], set: new Set() },
];
for (const { types: e4, set: t4 } of allExpandedTypes) {
  for (const r4 of e4) {
    const e5 = FLIPPED[r4];
    e5 ? e5.forEach(t4.add, t4) : t4.add(r4);
  }
}
console.log("sizes:", allExpandedTypes[0].set.size, allExpandedTypes[1].set.size);

const s = new Set<string>();
["a", "b"].forEach(s.add, s);
const t4: any = new Set();
["a"].forEach(t4.add, t4);
const add = t4.add;
add.call(t4, "q");
const rows: any[] = [{ set: new Set() }];
for (const { set: r } of rows) ["a"].forEach(r.add, r);
const e5t: any = ["z1", "z2"];
const t5: any = new Set();
e5t ? e5t.forEach(t5.add, t5) : t5.add("z");
console.log("anchors:", s.size, t4.size, rows[0].set.size, t5.size);

const SRC: any = { A: ["x", "y"] };
const KEYS = ["A"];
const dyn = SRC[KEYS[0]];
const ds: any = new Set();
dyn.forEach(ds.add, ds);
const m: any = new Map();
dyn.forEach(m.set, m);
const a2: any = [];
dyn.forEach(a2.push, a2);
console.log("dynamic:", ds.size, m.size, m.get("x"), m.get("y"), a2.length);

const marker = { tag: "T" };
const seen: boolean[] = [];
function observe(this: any): boolean {
  seen.push(this === marker);
  return true;
}
dyn.forEach(observe, marker);
dyn.map(observe, marker);
dyn.filter(observe, marker);
dyn.every(observe, marker);
dyn.some(function (this: any) { seen.push(this === marker); return false; }, marker);
dyn.find(function (this: any) { seen.push(this === marker); return false; }, marker);
dyn.findIndex(function (this: any) { seen.push(this === marker); return false; }, marker);
dyn.findLast(function (this: any) { seen.push(this === marker); return false; }, marker);
dyn.findLastIndex(function (this: any) { seen.push(this === marker); return false; }, marker);
console.log("family this-bound:", seen.length, seen.every(Boolean));
"#,
    );
    assert_eq!(
        stdout,
        "sizes: 3 1\n\
         anchors: 2 2 1 2\n\
         dynamic: 2 2 0 1 6\n\
         family this-bound: 18 true\n"
    );
}

/// The brand-check TypeError cases where node DOES throw — message-identical,
/// including V8's `NoSideEffectsToString` receiver rendering.
#[test]
fn incompatible_receiver_message_matches_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function thrown(fn: () => void): string {
  try {
    fn();
    return "NO_THROW";
  } catch (e: any) {
    return e.constructor.name + ": " + e.message;
  }
}
const SRC: any = { A: ["x"] };
const KEYS = ["A"];
const dyn = SRC[KEYS[0]];
const t4: any = new Set();
const m: any = new Map();
console.log("undef:", thrown(() => dyn.forEach(t4.add)));
console.log("obj:", thrown(() => dyn.forEach(t4.add, {})));
console.log("cross:", thrown(() => dyn.forEach(m.set, t4)));
console.log("num:", thrown(() => dyn.forEach(t4.add, 5.5 as any)));
console.log("str:", thrown(() => dyn.forEach(t4.add, "abc" as any)));
console.log("null:", thrown(() => (Set.prototype.add as any).call(null, 1)));
class Foo {}
console.log("inst:", thrown(() => (Set.prototype.add as any).call(new Foo(), 1)));
console.log("arr:", thrown(() => (Set.prototype.add as any).call([1, 2], 1)));
"#,
    );
    assert_eq!(
        stdout,
        "undef: TypeError: Method Set.prototype.add called on incompatible receiver undefined\n\
         obj: TypeError: Method Set.prototype.add called on incompatible receiver #<Object>\n\
         cross: TypeError: Method Map.prototype.set called on incompatible receiver #<Set>\n\
         num: TypeError: Method Set.prototype.add called on incompatible receiver 5.5\n\
         str: TypeError: Method Set.prototype.add called on incompatible receiver abc\n\
         null: TypeError: Method Set.prototype.add called on incompatible receiver null\n\
         inst: TypeError: Method Set.prototype.add called on incompatible receiver #<Foo>\n\
         arr: TypeError: Method Set.prototype.add called on incompatible receiver [object Array]\n"
    );
}
