//! Regression tests for #6935 — raw receivers and raw *stored values* held
//! across GC-capable property-key coercions.
//!
//! `ToPropertyKey(key)` runs a user `Symbol.toPrimitive` / `toString` /
//! `valueOf`, and even for a primitive key it allocates the stringified form.
//! Either can trigger a GC that **evacuates** (moves) live objects. Pre-fix the
//! property-key entry points held the receiver — and, on the set paths, the
//! value being written *into* it — as raw `f64` / raw pointer Rust locals:
//!
//! ```ignore
//! let key = js_to_property_key(key_value);        // user JS -> allocate -> GC
//! let obj = extract_obj_ptr(obj_value);           // receiver was raw across it
//! js_object_set_field_by_name(obj, key_str, value);   // stale receiver AND value
//! ```
//!
//! A Rust local is neither a GC root nor a shadow slot. This is strictly worse
//! than the #6655 operator family: there a stale operand produced one wrong
//! answer, whereas here the stale `value` is **written into a live object**, so
//! the dangling pointer outlives the call.
//!
//! The programs run with `PERRY_GC_FORCE_EVACUATE=1` (stress-copies every
//! marked non-pinned nursery object) and `PERRY_GC_VERIFY_EVACUATION=1` (panics
//! if a live slot still points at a forwarded object). Every stored payload is
//! a heap object whose fields are read back **after a further collection**, so
//! a stale store shows up as a wrong field value rather than passing by luck.
//!
//! ## What this suite does and does not prove
//!
//! Be precise about this, because the suite passes on the PRE-fix runtime too.
//! No in-language configuration currently reaches the state the bug needs — a
//! *minor* cycle that evacuates while the raw runtime locals are unpinned:
//!
//! * The `gc()` hook these programs call runs a **full mark-sweep**. Evacuation
//!   is minor-only, so nothing moves (`PERRY_GC_DIAG=1` prints no
//!   `[gc-evac-policy]`/`[gc-copy-minor]` line for any cycle here), and a stale
//!   pointer is trivially still valid.
//! * `perry/gc`'s `minor()` does evacuate (diag shows `reason=force`,
//!   `moved_objects` in the thousands) but engages
//!   `ManualGcScanGuard::force_full_scan()` (#4977). The conservative stack scan
//!   then **pins exactly the raw receiver/value locals this bug is about**, so
//!   it is masked by construction.
//! * `minor()` with `PERRY_CONSERVATIVE_STACK_SCAN=off` does evacuate them, but
//!   that combination is independently unsound on this build — a plain method's
//!   `this` (and even a `console.log` string literal) is lost across the
//!   collection, with or without the fix — so any failure it produces is
//!   uninterpretable.
//!
//! So these tests are a **behavioral guard**: they pin the observable semantics
//! of every rooted path (right value stored, right value read back, right key)
//! under the strongest GC stress the language surface can express today, and
//! they will start failing the day a minor-evacuating configuration becomes
//! reachable from compiled code. They are not evidence that the pre-fix runtime
//! was reproducibly wrong; the audit in #6935 is.
//!
//! Coverage: `js_object_{set,get}_property_key`,
//! `js_object_set_property_key_method`, `js_object_literal_set_computed`,
//! `js_object_define_accessor`, `js_dyn_index_{get,set}`,
//! `js_object_{get,set}_index_polymorphic`, `js_object_delete_dynamic`,
//! `js_array_{get,set}_index_or_string`, `js_native_call_method_value`,
//! `js_object_has_own` / `js_object_property_is_enumerable` (and their
//! `common_methods` method-call forms), `js_object_has_property`, and the proxy
//! `target_set` forward-to-target write.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run_forced_evacuation(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    // The runtime-only macOS link path does not pass `-framework CoreFoundation`,
    // but `perry-runtime` pulls `iana_time_zone`, which references `_CFRelease`
    // & co. Append the framework through the supported escape hatch so this
    // suite links regardless (same shim as the #6655 suite).
    let mut compile_cmd = Command::new(perry_bin());
    compile_cmd
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    if cfg!(target_os = "macos") {
        let extra = match std::env::var("PERRY_EXTRA_LINK_ARGS") {
            Ok(existing) if !existing.trim().is_empty() => {
                format!("{existing} -framework CoreFoundation")
            }
            _ => "-framework CoreFoundation".to_string(),
        };
        compile_cmd.env("PERRY_EXTRA_LINK_ARGS", extra);
    }
    let compile = compile_cmd.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed under forced evacuation (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Shared prelude: a key object whose `toString` churns the nursery and forces
/// a collection, plus a movable heap payload with readable fields so a stale
/// *stored* pointer is observable rather than coincidentally right.
const PRELUDE: &str = r#"
// Allocate a lot of short-lived nursery objects, force a collection, then
// refill: evacuation leaves the vacated region intact-but-dead, so a stale
// pointer read immediately after a copy usually still finds the original bytes.
// Re-filling gives that region a chance to be handed out and overwritten.
function churnAndCollect(): void {
  let sink = 0;
  for (let i = 0; i < 20000; i++) {
    const tmp = { i, s: "pad" + i };
    sink += tmp.s.length > 0 ? 1 : 0;
  }
  if (sink !== 20000) throw new Error("churn miscounted");
  (globalThis as any).gc?.();
  for (let i = 0; i < 20000; i++) {
    const tmp2 = { a: i, b: "fill" + i, c: [i, i + 1] };
    sink += tmp2.c[0] >= 0 ? 1 : 0;
  }
  if (sink !== 40000) throw new Error("refill miscounted");
}

// Keeps receivers / payloads reachable from a real GC root. An object reachable
// only through the (unrooted) raw local is DEAD at the collection, so it would
// merely be swept and a stale read might find intact bytes. Reachable objects
// are EVACUATED — the address genuinely changes and every rooted holder is
// rewritten, while the raw local is not.
const keepalive: any[] = [];

// A property key whose ToPropertyKey runs user JS that allocates and collects.
function heavyKey(name: string): any {
  const o: any = {
    n: name,
    toString(): string {
      churnAndCollect();
      return this.n;
    },
  };
  keepalive.push(o);
  return o;
}

// Same, via Symbol.toPrimitive rather than toString.
function heavyPrimKey(name: string): any {
  const o: any = { n: name };
  o[Symbol.toPrimitive] = function (): string {
    churnAndCollect();
    return this.n;
  };
  keepalive.push(o);
  return o;
}

// The STORED VALUE: a movable heap object carrying an identity we read back
// after a further collection. A stale stored pointer either reads the wrong
// tag/array or trips PERRY_GC_VERIFY_EVACUATION on the next cycle.
function payload(tag: number): any {
  const o: any = { tag: tag, arr: [tag, tag + 1], s: "payload-" + tag };
  keepalive.push(o);
  return o;
}

// A fresh receiver that is rooted (so it is evacuated, not swept).
function receiver(): any {
  const o: any = { seed: 1 };
  keepalive.push(o);
  return o;
}

let failures = 0;
function check(name: string, got: any, want: any): void {
  if (got !== want) {
    failures++;
    console.log("FAIL " + name + " got=" + String(got) + " want=" + String(want));
  }
}
// Read every field of a stored payload back, so a stale pointer cannot hide
// behind one lucky word.
function checkPayload(name: string, got: any, tag: number): void {
  if (got === undefined || got === null) {
    failures++;
    console.log("FAIL " + name + " payload missing");
    return;
  }
  check(name + ".tag", got.tag, tag);
  check(name + ".arr0", got.arr[0], tag);
  check(name + ".arr1", got.arr[1], tag + 1);
  check(name + ".s", got.s, "payload-" + tag);
}
"#;

/// The corruption case: the value being STORED is held across the key coercion,
/// so pre-fix a dangling pointer was written into a live object and outlived
/// the call. Covers the dynamic-index / polymorphic-index / array / computed
/// object-literal write paths.
#[test]
fn property_key_stored_values_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// --- js_dyn_index_set / js_object_set_index_polymorphic: obj[heavyKey] = v ---
const o1: any = receiver();
o1[heavyKey("k1")] = payload(1);
churnAndCollect();
checkPayload("dyn-index-set", o1.k1, 1);

// Symbol.toPrimitive flavour of the same write.
const o2: any = receiver();
o2[heavyPrimKey("k2")] = payload(2);
churnAndCollect();
checkPayload("dyn-index-set-toprimitive", o2.k2, 2);

// --- js_object_literal_set_computed / js_object_set_property_key ---
const lit: any = { [heavyKey("k3")]: payload(3) };
keepalive.push(lit);
churnAndCollect();
checkPayload("object-literal-computed", lit.k3, 3);

// Several computed keys in one literal, so later coercions collect while the
// earlier entries are already installed.
const lit2: any = {
  [heavyKey("a")]: payload(10),
  [heavyKey("b")]: payload(11),
  [heavyKey("c")]: payload(12),
};
keepalive.push(lit2);
churnAndCollect();
checkPayload("object-literal-multi-a", lit2.a, 10);
checkPayload("object-literal-multi-b", lit2.b, 11);
checkPayload("object-literal-multi-c", lit2.c, 12);

// --- js_array_set_index_or_string: a boxed/object key on an ARRAY receiver ---
const arr: any[] = [0, 1, 2];
keepalive.push(arr);
(arr as any)[heavyKey("named")] = payload(4);
churnAndCollect();
checkPayload("array-string-key-set", (arr as any).named, 4);
check("array-elements-intact", arr[2], 2);

// A non-canonical NUMERIC key on an array also stringifies (allocates).
(arr as any)[4294967295] = payload(5);
churnAndCollect();
checkPayload("array-noncanonical-index", (arr as any)[4294967295], 5);

// --- class-ref receiver takes the INT32-tagged class-ref write arm, where
// the stored value (not the receiver) is the operand at risk.
class C { static s(): number { return 1; } }
(C as any)[heavyKey("statKey")] = payload(6);
churnAndCollect();
checkPayload("class-static-computed", (C as any).statKey, 6);

// --- a write onto a prototype object, read back down the chain ---
const proto: any = receiver();
const child: any = Object.create(proto);
keepalive.push(child);
proto[heavyKey("inherited")] = payload(7);
churnAndCollect();
checkPayload("prototype-chain-computed", child.inherited, 7);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "a property-key write stored a stale value under forced evacuation"
    );
}

/// Receiver-only paths: reads, deletes, method dispatch and the own-property
/// predicates. A stale receiver here produces a wrong answer (or a fault)
/// rather than heap corruption, but it is the same rooting gap.
#[test]
fn property_key_receivers_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// --- js_dyn_index_get / js_object_get_index_polymorphic ---
const src: any = receiver();
src.hit = payload(20);
src.n = 7;
churnAndCollect();
checkPayload("dyn-index-get", src[heavyKey("hit")], 20);
check("dyn-index-get-number", src[heavyKey("n")], 7);

// --- js_array_get_index_or_string ---
const arr: any[] = [11, 12, 13];
keepalive.push(arr);
(arr as any).tail = payload(21);
churnAndCollect();
checkPayload("array-get-string-key", (arr as any)[heavyKey("tail")], 21);
check("array-get-numeric-key", arr[heavyKey("1") as any], 12);

// --- js_object_delete_dynamic ---
const del: any = receiver();
del.gone = payload(22);
del.kept = payload(23);
churnAndCollect();
delete del[heavyKey("gone")];
churnAndCollect();
check("delete-removed", del.gone, undefined);
checkPayload("delete-kept-sibling", del.kept, 23);

// --- js_native_call_method_value: obj[heavyKey]() ---
const callee: any = receiver();
callee.answer = function (): number { return this.seed + 41; };
churnAndCollect();
check("method-by-computed-key", callee[heavyKey("answer")](), 42);

// --- js_object_set_property_key_method: { [k]() {} } ---
const withMethod: any = {
  [heavyKey("run")](): number { return 99; },
};
keepalive.push(withMethod);
churnAndCollect();
check("computed-method-literal", withMethod.run(), 99);

// --- js_object_define_accessor: { get [k]() {}, set [k](v) {} } ---
let stored: any = null;
const accessor: any = {
  get [heavyKey("prop")](): any { return stored; },
  set [heavyKey("prop")](v: any) { stored = v; },
};
keepalive.push(accessor);
accessor.prop = payload(24);
churnAndCollect();
checkPayload("computed-accessor", accessor.prop, 24);

// --- hasOwn / propertyIsEnumerable / `in` ---
const probe: any = receiver();
probe.here = payload(25);
churnAndCollect();
check("hasOwnProperty-method", probe.hasOwnProperty(heavyKey("here")), true);
check("hasOwnProperty-miss", probe.hasOwnProperty(heavyKey("absent")), false);
check("Object.hasOwn", Object.hasOwn(probe, heavyKey("here")), true);
check(
  "propertyIsEnumerable",
  probe.propertyIsEnumerable(heavyKey("here")),
  true
);
// NOTE: `objectKey in obj` is deliberately NOT asserted here — Perry's
// `js_object_has_property` only runs ToPropertyKey for NUMBER keys, so an
// object key never reaches the coercion at all. That is a pre-existing spec
// gap (Node returns true), tracked separately; it is not a rooting bug.
// `in` with a NUMBER key does coerce (and allocates) with the receiver raw,
// which is the arm this suite covers.
const numKeyed: any = receiver();
numKeyed[307] = payload(26);
churnAndCollect();
check("in-operator-number-key", 307 in numKeyed, true);
checkPayload("number-key-payload", numKeyed[307], 26);

// The receiver must be intact after all of that.
checkPayload("probe-still-intact", probe.here, 25);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "a property-key read/dispatch path used a stale receiver under forced evacuation"
    );
}

/// Proxy forward-to-target writes (`target_set`) run `ToPropertyKey` with the
/// target receiver and the stored value both live. `target_get` was already
/// rooted; its write sibling was not.
#[test]
fn proxy_target_set_survives_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// No `set` trap -> the proxy forwards to the target through `target_set`,
// which performs the GC-capable ToPropertyKey with `target` and `value` live.
const target: any = receiver();
const p: any = new Proxy(target, {});
keepalive.push(p);
p[heavyKey("viaProxy")] = payload(30);
churnAndCollect();
checkPayload("proxy-forward-set", target.viaProxy, 30);
checkPayload("proxy-forward-read", p.viaProxy, 30);

// Reflect.set with an object key takes the same path.
const target2: any = receiver();
Reflect.set(target2, heavyKey("viaReflect"), payload(31));
churnAndCollect();
checkPayload("reflect-set", target2.viaReflect, 31);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "a proxy forward-to-target write used stale operands under forced evacuation"
    );
}
