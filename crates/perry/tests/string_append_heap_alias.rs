//! Regression: a string built with the in-place `+=` fast path
//! (`js_string_append`, which mutates a refcount==1 buffer in place) must not
//! corrupt an alias of that string that has escaped into the GC heap — an
//! object field or array element.
//!
//! Root cause: the in-place optimization is opt-out — a unique (refcount==1)
//! string is demoted to shared only at *some* alias sites. Codegen wired the
//! `let y = x` local-copy case but never the heap-store case, so a string
//! stored into an object field kept refcount==1 and a later `s += chunk`
//! rewrote the stored field in place.
//!
//! It manifests whenever code keeps a heap-stored snapshot of a string and then
//! keeps growing the source: `slot = s; s += chunk` leaves `slot` pointing at a
//! buffer the append rewrote, so a later equality check against the snapshot
//! wrongly sees the two as identical (and any read of the snapshot sees the
//! grown value).
//!
//! Fix: the write-barrier choke point `runtime_store_jsvalue_slot` demotes a
//! stored unique string to shared (dynamic/escaping object & array stores), and
//! the scalar-replaced field-store codegen mirrors the `let y = x` addref
//! (non-escaping object literals whose field stores are inlined).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> String {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// Escaping (heap-allocated, dynamic) object field — the snapshot-then-grow
/// shape. `o.v = s` routes through `js_object_set_field_by_name` →
/// `runtime_store_jsvalue_slot`, which must demote `s` to shared so the later
/// `s += "b"` allocates fresh instead of rewriting the stored field.
#[test]
fn unique_string_stored_in_dynamic_object_field_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
function makeBox(): { v: string } { return { v: "" }; } // escapes -> dynamic object
const o = makeBox();
let s = "";
s += "a";   // s is uniquely owned (refcount==1) after the in-place append
o.v = s;    // s escapes into a heap field — must be demoted to shared
s += "b";   // must NOT mutate o.v in place
console.log("o.v=" + o.v + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("o.v=a s=ab"),
        "string stored in a dynamic object field must not be corrupted by a later += (got: {out:?})"
    );
}

/// Non-escaping object literal whose field store is scalar-replaced / inlined by
/// codegen (bypasses the runtime write barrier). The scalar-field-store codegen
/// must emit the shared-demote.
///
/// The strings are deliberately NON-SSO (> 5 bytes) and the stored value is made
/// uniquely owned (refcount==1) via a first append on a shared literal — only
/// then does the later in-place `+=` corrupt the stored field without the fix.
/// (An earlier version used `""`/`"a"`, which are small-string-optimized and
/// carry no refcount, so it passed even with the codegen reverted.)
#[test]
fn unique_string_stored_in_scalar_replaced_field_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const o = { v: "" };  // never escapes -> scalar-replaced, field store inlined
let s = "prefix";     // 6 bytes -> heap (non-SSO), shared literal (refcount 0)
s += "_init";         // append on shared -> fresh heap string, refcount==1
o.v = s;              // scalar-replaced field store -> must demote s to shared
s += "_more";         // refcount==1 in-place append -> must NOT corrupt o.v
console.log("o.v=" + o.v + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("o.v=prefix_init s=prefix_init_more"),
        "string stored in a scalar-replaced object field must not be corrupted by a later += (got: {out:?})"
    );
}

// Array-element stores have the same aliasing exposure as object fields, across
// every store path: literal construction, `arr[i] = s`, `push`, and `splice`.
// Each stores a NON-SSO string made uniquely-owned (refcount==1) via a first
// append, then grows the source — the stored element must keep its value. These
// fail without the array-store demotes (`note_array_slot`, `js_array_from_values`,
// the splice insert, and the inline array-literal codegen).

/// `const a = [s]` — small literal (lowers to `js_array_alloc` + `js_array_push_f64`).
#[test]
fn unique_string_stored_in_array_literal_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let s = "prefix";   // non-SSO, shared literal
s += "_init";       // append on shared -> fresh heap string, refcount==1
const a = [s];      // element store -> must demote s to shared
s += "_more";       // refcount==1 in-place append -> must NOT corrupt a[0]
console.log("a0=" + a[0] + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("a0=prefix_init s=prefix_init_more"),
        "string stored in an array literal must not be corrupted by a later += (got: {out:?})"
    );
}

/// `a[i] = s` — index assignment.
#[test]
fn unique_string_stored_via_index_set_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const a = ["placeholder"];
let s = "prefix";
s += "_init";
a[0] = s;           // index-set -> must demote s to shared
s += "_more";
console.log("a0=" + a[0] + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("a0=prefix_init s=prefix_init_more"),
        "string stored via arr[i]= must not be corrupted by a later += (got: {out:?})"
    );
}

/// `a.push(s)` — push.
#[test]
fn unique_string_pushed_into_array_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const a = [];
let s = "prefix";
s += "_init";
a.push(s);          // push -> must demote s to shared
s += "_more";
console.log("a0=" + a[0] + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("a0=prefix_init s=prefix_init_more"),
        "string pushed into an array must not be corrupted by a later += (got: {out:?})"
    );
}

/// `a.splice(i, 0, s)` — splice insertion.
#[test]
fn unique_string_spliced_into_array_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const a = ["head", "tail"];
let s = "prefix";
s += "_init";
a.splice(1, 0, s);  // splice insert -> must demote s to shared
s += "_more";
console.log("a1=" + a[1] + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("a1=prefix_init s=prefix_init_more"),
        "string spliced into an array must not be corrupted by a later += (got: {out:?})"
    );
}

/// The OUTLINED array-literal construction path (`js_array_from_values`), forced
/// via `PERRY_FULL_OUTLINE_IC=1`. The array must escape so it isn't
/// scalar-replaced and routes through `lower_array_literal`'s outline branch.
#[test]
fn unique_string_in_outlined_array_literal_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let s = "prefix";
s += "_init";
const a = [s];                // escapes below -> js_array_from_values (full-outline)
s += "_more";
(globalThis as any).keep = a; // escape so the literal isn't scalar-replaced
console.log("a0=" + a[0] + " s=" + s);
"#,
    )
    .expect("write entry");
    let output = dir.path().join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .env("PERRY_FULL_OUTLINE_IC", "1")
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
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
    let out = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(
        out.contains("a0=prefix_init s=prefix_init_more"),
        "string in an outlined array literal must not be corrupted by a later += (got: {out:?})"
    );
}

// #5552: sibling array insert/replace paths that #5548 left uncovered —
// `unshift`, `fill`, `with`, and mixed-type literal construction
// (`js_array_from_jsvalue`). Each does a raw write of the source value into a
// slot without the demote; the same snapshot-then-grow shape corrupts the
// stored element. Each fails without the matching runtime demote.

/// `a.unshift(s)` — front insertion (`js_array_unshift_f64`).
#[test]
fn unique_string_unshifted_into_array_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let s = "prefix";   // non-SSO, shared literal
s += "_init";       // append on shared -> fresh heap string, refcount==1
const a = ["x"];
a.unshift(s);       // front insert -> must demote s to shared
s += "_more";       // refcount==1 in-place append -> must NOT corrupt a[0]
console.log("a0=" + a[0] + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("a0=prefix_init s=prefix_init_more"),
        "string unshifted into an array must not be corrupted by a later += (got: {out:?})"
    );
}

/// `a.fill(s)` — fills every slot with the same source (`js_array_fill`). The
/// source aliases the source local AND every filled slot, so a later `+=` must
/// not corrupt any of them.
#[test]
fn unique_string_filled_into_array_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let s = "prefix";
s += "_init";
const a = ["x", "y", "z"];
a.fill(s);          // fill all slots -> must demote s to shared
s += "_more";
console.log("a0=" + a[0] + " a2=" + a[2] + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("a0=prefix_init a2=prefix_init s=prefix_init_more"),
        "string filled into an array must not be corrupted by a later += (got: {out:?})"
    );
}

/// `a.with(i, s)` — immutable replace stores the source into the new array's
/// slot (`js_array_with`).
#[test]
fn unique_string_stored_via_with_is_not_corrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let s = "prefix";
s += "_init";
const a = ["placeholder"].with(0, s);  // replace -> must demote s to shared
s += "_more";
console.log("a0=" + a[0] + " s=" + s);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("a0=prefix_init s=prefix_init_more"),
        "string stored via arr.with() must not be corrupted by a later += (got: {out:?})"
    );
}

// NOTE: the mixed-type construction path `js_array_from_jsvalue` (#5552) is not
// emitted by codegen from any TypeScript source, so it has no compiled-TS
// regression test here. Its demote is covered by the runtime unit test
// `test_array_from_jsvalue_demotes_unique_string_against_inplace_append` in
// crates/perry-runtime/src/array/tests.rs.

/// The snapshot-then-grow shape end to end: store the latest value into a heap
/// field each step, then keep growing the source. Every stored snapshot must
/// retain the value it had when stored (no in-place rewrite).
#[test]
fn stored_snapshots_retain_their_value_across_further_appends() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
function makeBox(): { last: string } { return { last: "" }; }
const a = makeBox();
const b = makeBox();
let cur = "";
cur += "x"; a.last = cur;   // a.last = "x"
cur += "y"; b.last = cur;   // b.last = "xy", a.last must still be "x"
console.log("a=" + a.last + " b=" + b.last + " cur=" + cur);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("a=x b=xy cur=xy"),
        "earlier stored snapshot must not be advanced by later appends (got: {out:?})"
    );
}

/// Control: the in-place append optimization must stay correct for the
/// non-escaping build-loop case it targets (no aliases to corrupt).
#[test]
fn non_escaping_build_loop_still_correct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let s = "";
for (let i = 0; i < 5; i++) s += "x";
console.log("s=" + s + " len=" + s.length);
"#,
    )
    .expect("write entry");
    let out = compile_and_run(dir.path(), &entry);
    assert!(
        out.contains("s=xxxxx len=5"),
        "non-escaping build loop must remain correct (got: {out:?})"
    );
}
