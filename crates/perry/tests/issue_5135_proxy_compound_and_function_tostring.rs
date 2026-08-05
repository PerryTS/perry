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

/// #6908 §1: the remaining `Array.prototype` mutators — `reverse` / `splice` /
/// `fill` / `copyWithin` — on a Proxy receiver run their spec loops through
/// the traps instead of silently falling through to `undefined` (`fill` /
/// `copyWithin` were additionally noop-backed on the prototype, so even the
/// method value was inert). `sort` already worked via the array-like engine;
/// pinned here so it stays that way. All expectations byte-identical to
/// `node --experimental-strip-types` (v26.5.1).
#[test]
fn proxy_array_remaining_mutators_route_through_traps() {
    let out = compile_and_run(
        r#"
{
  const t: any = [3, 1, 2];
  const p: any = new Proxy(t, {});
  const r = p.reverse();
  console.log("reverse:", t.join(","), r === p);
}
{
  // Even length with a hole: the (lower-exists, upper-missing) case runs
  // DeletePropertyOrThrow on the lower side.
  const t: any = [1, , 3, 4];
  const p: any = new Proxy(t, {});
  p.reverse();
  console.log("reverse-hole:", t.join(","), 0 in t, 1 in t, 2 in t, 3 in t);
}
{
  const t: any = [10, 9, 1];
  const p: any = new Proxy(t, {});
  const r = p.sort();
  console.log("sort:", t.join(","), r === p);
}
{
  const t: any = [1, 2, 3, 4, 5];
  const p: any = new Proxy(t, {});
  const removed = p.splice(1, 2, "a", "b", "c");
  console.log("splice:", t.join(","), t.length, Array.isArray(removed), removed.join(","));
}
{
  // One-arg form deletes to the end; holes stay holes in the removed array.
  const t: any = [1, , 3];
  const p: any = new Proxy(t, {});
  const removed = p.splice(1);
  console.log("splice-1arg:", t.join(","), t.length, removed.length, 0 in removed, 1 in removed);
}
{
  const t: any = [1, 2, 3, 4];
  const p: any = new Proxy(t, {});
  const r = p.fill(7, 1, 3);
  console.log("fill:", t.join(","), r === p);
}
{
  const t: any = [1, 2, 3, 4, 5];
  const p: any = new Proxy(t, {});
  const r = p.copyWithin(0, 3);
  console.log("copyWithin:", t.join(","), r === p);
}
"#,
    );
    assert_eq!(
        out,
        "reverse: 2,1,3 true\n\
         reverse-hole: 4,3,,1 true true false true\n\
         sort: 1,10,9 true\n\
         splice: 1,a,b,c,4,5 6 true 2,3\n\
         splice-1arg: 1 1 2 false true\n\
         fill: 1,7,7,4 true\n\
         copyWithin: 4,5,3,4,5 true\n",
        "proxy reverse/sort/splice/fill/copyWithin must mutate through the traps and return per spec"
    );
}

/// #6908 §1 trap-order pin: the spec loops drive `set` / `deleteProperty` in
/// a defined sequence, and a `deleteProperty` trap returning false throws the
/// spec TypeError BEFORE the length write. Byte-identical to node.
#[test]
fn proxy_array_remaining_mutators_fire_traps_in_spec_order() {
    let out = compile_and_run(
        r#"
{
  const log: string[] = [];
  const t: any = [1, 2, 3, 4];
  const p: any = new Proxy(t, {
    get(o: any, k: any, rc: any) {
      if (typeof k === "string") log.push("g" + k);
      return Reflect.get(o, k, rc);
    },
    set(o: any, k: any, v: any, rc: any) {
      log.push("s" + k + "=" + v);
      return Reflect.set(o, k, v, rc);
    },
  });
  p.reverse();
  console.log("reverse-traps:", t.join(","), log.join("|"));
}
{
  const log: string[] = [];
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {
    set(o: any, k: any, v: any, rc: any) {
      log.push("s" + k + "=" + v);
      return Reflect.set(o, k, v, rc);
    },
    deleteProperty(o: any, k: any) {
      log.push("d" + k);
      return Reflect.deleteProperty(o, k);
    },
  });
  const removed = p.splice(0, 1);
  console.log("splice-traps:", t.join(","), removed.join(","), log.join("|"));
}
{
  const log: string[] = [];
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {
    set(o: any, k: any, v: any, rc: any) {
      log.push("s" + k + "=" + v);
      return Reflect.set(o, k, v, rc);
    },
  });
  p.fill(0, 1);
  console.log("fill-traps:", t.join(","), log.join("|"));
}
{
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, { deleteProperty() { return false; } });
  try { p.splice(0, 1); console.log("splice-refused: NO-THROW"); }
  catch (e: any) { console.log("splice-refused:", (e instanceof TypeError) ? "TypeError" : "other"); }
  console.log("splice-refused-after:", t.join(","), t.length);
}
"#,
    );
    assert_eq!(
        out,
        "reverse-traps: 4,3,2,1 greverse|glength|g0|g3|s0=4|s3=1|g1|g2|s1=3|s2=2\n\
         splice-traps: 2,3 1 s0=2|s1=3|d2|slength=2\n\
         fill-traps: 1,0,0 s1=0|s2=0\n\
         splice-refused: TypeError\n\
         splice-refused-after: 2,3,3 3\n",
        "proxy mutator trap sequences must match the spec algorithms"
    );
}

/// #6908 §1, `.call` forms: `Array.prototype.<m>.call(proxy, …)` lowers to
/// the value-generic engines (`js_array_reverse_value` / `js_arraylike_splice`
/// / `js_array_fill_generic` / `js_array_copy_within_value`), which must
/// route a Proxy receiver to the same trap loops as the member call.
#[test]
fn proxy_array_mutator_call_forms_route_through_traps() {
    let out = compile_and_run(
        r#"
{
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {});
  Array.prototype.reverse.call(p);
  console.log("call-reverse:", t.join(","));
}
{
  const t: any = [1, 2, 3, 4];
  const p: any = new Proxy(t, {});
  const rm: any = Array.prototype.splice.call(p, 1, 2);
  console.log("call-splice:", rm.join(","), t.join(","));
}
{
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {});
  Array.prototype.fill.call(p, 8, 1);
  console.log("call-fill:", t.join(","));
}
{
  const t: any = [1, 2, 3, 4];
  const p: any = new Proxy(t, {});
  Array.prototype.copyWithin.call(p, 0, 2);
  console.log("call-copyWithin:", t.join(","));
}
"#,
    );
    assert_eq!(
        out,
        "call-reverse: 3,2,1\ncall-splice: 2,3 1,4\ncall-fill: 1,8,8\ncall-copyWithin: 3,4,3,4\n",
        "Array.prototype.<m>.call(proxy, ...) must fire the proxy traps"
    );
}

/// #6908 §2: a prototype mutator thunk invoked as a plain value has no
/// receiver (`IMPLICIT_THIS` is undefined) — spec step 1 `ToObject(this)`
/// throws TypeError. Previously this silently no-opped, the worst outcome:
/// callers observed neither the mutation nor an error. Also pins that a
/// PRIOR method call's receiver does not leak into the bare call.
#[test]
fn receiverless_prototype_mutator_thunk_throws_type_error() {
    let out = compile_and_run(
        r#"
{
  const t: any = [1, 2];
  const f: any = t["push"];
  console.log("typeof:", typeof f);
  try { f(3); console.log("bare-push: NO-THROW"); }
  catch (e: any) { console.log("bare-push:", (e instanceof TypeError) ? "TypeError" : "other", e.message); }
  console.log("bare-push-after:", t.join(","));
}
{
  const t: any = [1, 2];
  const p: any = new Proxy(t, {});
  const g: any = p.push;
  try { g(3); console.log("proxy-bare-push: NO-THROW"); }
  catch (e: any) { console.log("proxy-bare-push:", (e instanceof TypeError) ? "TypeError" : "other"); }
  console.log("proxy-bare-push-after:", t.join(","));
}
{
  // A previous method-style call must not leave its receiver armed.
  const t: any = [1];
  t.push(9);
  const f: any = t["pop"];
  try { f(); console.log("stale-this: NO-THROW"); }
  catch (e: any) { console.log("stale-this:", (e instanceof TypeError) ? "TypeError" : "other"); }
  console.log("stale-this-after:", t.join(","));
}
"#,
    );
    assert_eq!(
        out,
        "typeof: function\n\
         bare-push: TypeError Cannot convert undefined or null to object\n\
         bare-push-after: 1,2\n\
         proxy-bare-push: TypeError\n\
         proxy-bare-push-after: 1,2\n\
         stale-this: TypeError\n\
         stale-this-after: 1,9\n",
        "a receiver-less builtin mutator thunk must throw the spec TypeError, not silently no-op"
    );
}
