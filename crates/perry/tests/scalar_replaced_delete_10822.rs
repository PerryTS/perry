//! Regression: `delete o.k` on a NON-ESCAPING receiver must actually remove
//! the property — a later read must answer `undefined`, not the deleted value.
//!
//! Issue #10822. `const o = { a: 1, c: 3 }; delete o.c; String(o.c)` printed
//! `"3"`. Not "the property is still there": the read returned the value the
//! property held *before* the delete, `o.c === undefined` was `false`, and
//! nothing in the output said anything was wrong.
//!
//! The mechanism is escape analysis plus scalar replacement, not the delete
//! representation (it reproduced identically with `PERRY_OBJECT_TOMBSTONES=0`)
//! and not the `Ptr<Shape>` proven path (`--opt-report` says `0 selected /
//! 1 denied`: ptr-shape rule 5 disables the whole module on a `delete`).
//!
//! `collectors/escape_check.rs` stripped `Expr::Delete` in its generic unary
//! arm and handed the inner `PropertyGet` to the "plain declared-field read —
//! safe" arm, which returns early WITHOUT visiting the bare `LocalGet`. The
//! receiver therefore never escaped: `stmt/let_stmt.rs` scalar-replaced it,
//! elided the heap object entirely, and gave each observed field its own
//! alloca. The delete still emitted
//! `js_object_delete_field_value(<dummy slot>, "c")` against the
//! never-populated `ctx.locals[id]` alloca, where the deliberate "a primitive
//! receiver no-ops to `true`" guard made it a silent nothing — and the read
//! loaded the `c` alloca, which still held `3`.
//!
//! That also explains the fragility in the report. Every "repair" —
//! `Object.keys(o)`, `JSON.stringify(o)`, `objs.push(o)`, `"c" in o` — passes
//! `o` somewhere as a value, reaching the bare-`LocalGet` arm that escapes the
//! candidate. A `delete` alone never did.
//!
//! This is the REMOVAL sibling of the read rule (#10689) and the write rules
//! (#9024 `PropertySet`/`PutValueSet`, #9460 `PropertyUpdate`).
//!
//! Fixtures are `.js` so `delete` of a plain key and a holey array literal are
//! valid source with no `as any` casts; a cast on the receiver would route the
//! access through the dynamic path and miss the lowering the bug lived in.
//! The two "masking" programs are pinned as tests of their own: they were
//! already correct before the fix, so they are the ones that tell "fixed"
//! apart from "masked".

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Write `entry` into `dir`, compile it with `--no-cache`
/// `PERRY_NO_AUTO_OPTIMIZE=1` (links the prebuilt runtime archive), run it, and
/// return stdout. Mirrors the helper in `object_prototype_value_read_10689.rs`.
fn compile_and_run_js(dir: &Path, entry: &str, source: &str) -> String {
    let entry_path = dir.join(entry);
    std::fs::write(&entry_path, source).expect("write fixture");
    let output = dir.join(format!("{entry}.bin"));

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry_path)
        .arg("--no-cache")
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
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

fn run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    compile_and_run_js(dir.path(), "main.js", source)
}

/// The reported program, byte for byte, plus the two other ways the same
/// single read is spelled. Before the fix: `3`, `false`, `number`.
#[test]
fn bare_read_after_delete_is_undefined() {
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
delete o.c;
console.log(String(o.c));
"#),
        "undefined\n"
    );
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
delete o.c;
console.log(String(o.c === undefined));
"#),
        "true\n"
    );
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
delete o.c;
console.log(typeof o.c);
"#),
        "undefined\n"
    );
}

/// The read is not special to `String()`. Arithmetic on the deleted value
/// answered `4` before the fix, so a checksum over such a loop was silently
/// off rather than throwing.
#[test]
fn deleted_field_used_arithmetically_is_nan() {
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
delete o.c;
const v = o.c;
console.log(String(v + 1));
"#),
        "NaN\n"
    );
}

/// Returned from a function and passed as a call argument — the two ways the
/// value leaves the frame. Both answered `3` before the fix.
#[test]
fn deleted_field_returned_and_passed() {
    assert_eq!(
        run(r#"
function f() {
  const o = { a: 1, c: 3 };
  delete o.c;
  return o.c;
}
console.log(String(f()));
"#),
        "undefined\n"
    );
    assert_eq!(
        run(r#"
function show(x) { return String(x); }
const o = { a: 1, c: 3 };
delete o.c;
console.log(show(o.c));
"#),
        "undefined\n"
    );
}

/// The loop form the bug was found in: the checksum was off by exactly the
/// trip count. Printed `5` before the fix.
#[test]
fn delete_inside_a_loop() {
    assert_eq!(
        run(r#"
let bad = 0;
for (let i = 0; i < 5; i++) {
  const o = { a: i, b: i + 1, c: i + 2, d: i + 3 };
  delete o.c;
  const v = o.c;
  if (v !== undefined) bad++;
}
console.log(String(bad));
"#),
        "0\n"
    );
}

/// Not an object-literal-only defect: a `new C()` whose binding never escapes
/// is the same scalar-replacement candidate. Printed `3` before the fix.
#[test]
fn class_instance_delete() {
    assert_eq!(
        run(r#"
class C {
  constructor() { this.a = 1; this.c = 3; }
}
const o = new C();
delete o.c;
console.log(String(o.c));
"#),
        "undefined\n"
    );
}

/// Slot position does not matter: the first field of a small literal and a
/// late field of a wide one behaved identically (`1` and `10` before the fix).
#[test]
fn delete_first_field_and_late_field() {
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
delete o.a;
console.log(String(o.a));
"#),
        "undefined\n"
    );
    assert_eq!(
        run(r#"
const o = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9, j: 10, k: 11, l: 12 };
delete o.j;
console.log(String(o.j));
"#),
        "undefined\n"
    );
}

/// A computed key reaches the same receiver through `Expr::IndexGet`, which
/// the same arm has to cover. Printed `3` before the fix.
#[test]
fn computed_key_delete() {
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
const k = "c";
delete o[k];
console.log(String(o.c));
"#),
        "undefined\n"
    );
}

/// The sibling this fix found: a non-escaping ARRAY literal is scalar-replaced
/// by the same machinery (`collectors/escape_arrays.rs`), and
/// `delete a[1]` read back `2` before the fix. Not in the original report.
#[test]
fn delete_array_element() {
    assert_eq!(
        run(r#"
const a = [1, 2, 3];
delete a[1];
console.log(String(a[1]));
console.log(String(a.length));
"#),
        "undefined\n3\n"
    );
}

/// The masking programs. These were ALREADY correct before the fix — a run
/// that only checked the broken shapes could not tell a real fix from a
/// change that merely forces every object onto the heap path, and these are
/// what keep the difference visible.
#[test]
fn second_observer_variants_stay_correct() {
    // A later `Object.keys` repaired the read by making `o` escape.
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
delete o.c;
console.log(String(o.c));
console.log(Object.keys(o).join(","));
"#),
        "undefined\na\n"
    );
    // So did handing the object to something else.
    assert_eq!(
        run(r#"
const objs = [];
const o = { a: 1, c: 3 };
objs.push(o);
delete o.c;
console.log(String(o.c));
"#),
        "undefined\n"
    );
}

/// The observations that were right even while the read was wrong: they do
/// not go through the scalar field load, and each of them makes the receiver
/// escape on its own. They must stay right.
#[test]
fn key_observations_stay_correct() {
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
delete o.c;
console.log(String("c" in o));
console.log(Object.keys(o).join(","));
console.log(JSON.stringify(o));
"#),
        "false\na\n{\"a\":1}\n"
    );
}

/// A deleted key that is written again is present again, with the new value.
#[test]
fn delete_then_readd() {
    assert_eq!(
        run(r#"
const o = { a: 1, c: 3 };
delete o.c;
o.c = 99;
console.log(String(o.c));
console.log(Object.keys(o).join(","));
"#),
        "99\na,c\n"
    );
}
