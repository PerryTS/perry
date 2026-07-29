//! Regression tests for #5135: importing `immer` and calling
//! `produce(base, draft => { draft.count++; draft.list.push(3) })` crashed with
//! SIGSEGV. immer's drafts are `Proxy` objects that are statically typed as the
//! plain base type, which exposed three independent Perry bugs. These tests
//! reproduce each with a plain `Proxy` (no immer dependency needed):
//!
//!  1. A compound-assignment write through a `Proxy` (`p.count++`) lowered to
//!     `js_object_set_field_by_name` with the proxy's NaN-box tag masked off;
//!     the runtime had no proxy branch there and dereferenced the masked id as
//!     an `ObjectHeader` → SIGSEGV. (The read side already routed proxies.)
//!  2. The statically-typed `Function.toString` static-member read collapsed to
//!     `globalThis.toString` and folded to a number, so
//!     `Function.toString.call(Ctor)` (immer's `isPlainObject`) threw
//!     "Function.prototype.call was called on a value that is not a function".
//!  3. A native array method / `length` read on a value that is a `Proxy` at
//!     runtime (`draft.list.push(x)`) dereferenced the masked proxy id as an
//!     `ArrayHeader` → SIGSEGV. The array helpers now route a proxy receiver
//!     through its traps.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.ts");
    let output = root.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(root)
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (signal/exit) — likely a SIGSEGV regression\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// Fix #1: `proxy.count++` writes through the `set` trap instead of crashing.
#[test]
fn proxy_compound_assignment_routes_through_set_trap() {
    let out = compile_and_run(
        r#"
const target: any = { count: 0 };
const p: any = new Proxy(target, {
  get(t: any, k: any) { return t[k]; },
  set(t: any, k: any, v: any) { t[k] = v; return true; },
});
p.count++;
console.log(p.count, target.count);
"#,
    );
    assert_eq!(
        out, "1 1\n",
        "p.count++ must write through the proxy set trap"
    );
}

/// Fix #2: `Function.toString` (and `Array.toString`) read as a value are real
/// functions, not numbers.
#[test]
fn function_tostring_static_member_is_callable() {
    let out = compile_and_run(
        r#"
console.log(typeof Function.toString);
console.log(typeof Array.toString);
// immer's isPlainObject reaches `Function.toString.call(Ctor)`:
console.log(typeof Function.toString.call(Array));
"#,
    );
    assert_eq!(
        out, "function\nfunction\nstring\n",
        "Function.toString / Array.toString must resolve to callable functions"
    );
}

/// Fix #3: a native array method (`push`) on a value that is a Proxy at runtime
/// dispatches through the proxy's traps instead of dereferencing the masked
/// proxy id as an ArrayHeader. This mirrors immer's `draft.list.push(x)`, where
/// the receiver is a member access (`obj.list`) that returns a proxy array — the
/// `js_array_push_f64` runtime helper path the issue actually exercised.
#[test]
fn proxy_array_push_via_member_routes_through_traps() {
    let out = compile_and_run(
        r#"
const target: any = [1, 2];
const inner: any = new Proxy(target, {
  get(t: any, k: any) { return t[k]; },
  set(t: any, k: any, v: any) { t[k] = v; return true; },
});
const holder: any = { list: inner };
holder.list.push(3);
console.log(target.join(","), holder.list.length);
"#,
    );
    assert_eq!(
        out, "1,2,3 3\n",
        "obj.list.push must mutate the proxied array through its set trap"
    );
}

/// The dispatch-tower variant of fix #3: when the receiver is untyped, #6397
/// defers `proxy.push(x)` to `js_native_call_method`, whose proxy branch
/// resolves the method via `Get(proxy, "push")` and lands in the
/// `Array.prototype` thunk → `array_proto_mutator`. That normalization had no
/// proxy branch (`as_real_array` rightly rejects handle-band ids;
/// `run_object_mutator` only accepts plain objects), so the whole mutation was
/// silently dropped — no trap fired, nothing mutated, `undefined` returned.
/// Pin the spec trap sequence byte-for-byte against node: push fires
/// `get(length)` then `set(<index>)` + `set(length)`, on top of the method
/// lookup's own `get(push)`.
#[test]
fn proxy_array_push_via_dispatch_fires_traps_in_spec_order() {
    let out = compile_and_run(
        r#"
let gets = 0, sets = 0;
const target: any = [1, 2];
const inner: any = new Proxy(target, {
  get(t: any, k: any) { gets++; return t[k]; },
  set(t: any, k: any, v: any) { sets++; t[k] = v; return true; },
});
const r0 = inner[0];
inner[5] = 42;
inner.push(9);
console.log("r0=" + r0, "t5=" + target[5], "t=" + target.join(","), "gets=" + gets, "sets=" + sets);
"#,
    );
    assert_eq!(
        out, "r0=1 t5=42 t=1,2,,,,42,9 gets=3 sets=3\n",
        "proxy push must fire get(length) + set(index) + set(length) traps and mutate the target"
    );
}

/// Same dispatch path with a default (empty-handler) Proxy: every trap
/// forwards to the target, so push must land on the target array.
#[test]
fn proxy_array_push_empty_handler_forwards_to_target() {
    let out = compile_and_run(
        r#"
const target: any = [1, 2];
const p: any = new Proxy(target, {});
p.push(3);
console.log(target.join(","), p.length);
"#,
    );
    assert_eq!(
        out, "1,2,3 3\n",
        "empty-handler proxy push must forward to the target"
    );
}

/// CodeRabbit follow-ups on the trap-routed mutators: holes must be preserved
/// (`HasProperty` gates each element move; a source hole DELETES the
/// destination instead of materializing an own `undefined`), and a
/// `deleteProperty` trap returning false must throw the spec TypeError BEFORE
/// the length write (DeletePropertyOrThrow). All expectations byte-identical
/// to `node --experimental-strip-types`.
#[test]
fn proxy_array_mutators_preserve_holes_and_throw_on_refused_delete() {
    let out = compile_and_run(
        r#"
{
  const t: any = [1, , 3];
  const p: any = new Proxy(t, {});
  const r = p.shift();
  console.log("shift:", r, t.join(","), t.length, (0 in t) ? "has0" : "hole0");
}
{
  const t: any = [7, 8];
  const p: any = new Proxy(t, {});
  const r = p.pop();
  console.log("pop:", r, t.join(","), t.length);
}
{
  const t: any = [5, , 6];
  const p: any = new Proxy(t, {});
  const r = p.unshift(0);
  console.log("unshift:", r, t.join(","), t.length, (2 in t) ? "has2" : "hole2");
}
{
  const t: any = [1, 2];
  const p: any = new Proxy(t, { deleteProperty() { return false; } });
  try { p.pop(); console.log("delthrow: NO-THROW"); }
  catch (e: any) { console.log("delthrow: threw", (e instanceof TypeError) ? "TypeError" : "other"); }
  console.log("delthrow-after:", t.join(","), t.length);
}
"#,
    );
    assert_eq!(
        out,
        "shift: 1 ,3 2 hole0\npop: 8 7 1\nunshift: 4 0,5,,6 4 hole2\ndelthrow: threw TypeError\ndelthrow-after: 1,2 2\n",
        "proxy mutators must preserve holes and DeletePropertyOrThrow"
    );
}
