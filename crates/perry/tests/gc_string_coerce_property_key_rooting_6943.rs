//! Regression tests for #6943 — raw receivers and raw *stored values* held
//! across a `js_string_coerce` call that is being used as the property-key
//! coercion.
//!
//! Third and last family in the unrooted-operand-across-GC-capable-coercion
//! series (#6655/#6934 dynamic arith, #6935/#6941 `ToPropertyKey`). These entry
//! points never call `js_to_property_key`; they stringify the key with
//! `js_string_coerce` directly:
//!
//! ```ignore
//! let obj = extract_obj_ptr(obj_value);              // receiver, raw local
//! let key_str = js_string_coerce(key_value);         // user JS -> allocate -> GC
//! own_key_present(obj, key_str);                     // stale receiver
//! ```
//!
//! `js_string_coerce` returns early — with no allocation at all — for an
//! already-heap `STRING_TAG` value. Every other shape allocates: an SSO short
//! string materializes onto the heap, a number/bool/null/BigInt builds its
//! stringification, and a `POINTER_TAG` object routes through
//! `js_jsvalue_to_string`, which invokes a user `toString` / `valueOf`. Any of
//! those can trigger a GC that **evacuates** live objects, and a Rust local is
//! neither a GC root nor a shadow slot.
//!
//! The programs run with `PERRY_GC_FORCE_EVACUATE=1` (stress-copies every
//! marked non-pinned nursery object) and `PERRY_GC_VERIFY_EVACUATION=1` (panics
//! if a live slot still points at a forwarded object). Stored payloads are heap
//! objects whose fields are read back **after a further collection**, so a
//! stale store shows up as a wrong field value rather than passing by luck.
//!
//! ## What this suite does and does not prove
//!
//! Same caveat as the #6935 suite, and it must not be overstated: **these tests
//! pass on the pre-fix runtime too.** No in-language configuration currently
//! reaches the state the bug needs — a *minor* cycle that evacuates while the
//! raw runtime locals are unpinned:
//!
//! * The `gc()` hook these programs call runs a **full mark-sweep**, and
//!   evacuation is minor-only, so nothing moves (#6946).
//! * `perry/gc`'s `minor()` does evacuate but engages
//!   `ManualGcScanGuard::force_full_scan()`, whose conservative stack scan pins
//!   exactly the raw receiver/value locals this bug is about (#4977, #6942).
//! * `minor()` + `PERRY_CONSERVATIVE_STACK_SCAN=off` evacuates them, but that
//!   combination is independently unsound on this build, so any failure it
//!   produces is uninterpretable. #6941's agent got an apparent repro that way
//!   and retracted it after a control reproduced with AND without the fix.
//!
//! So this is a **behavioral guard**: it pins the observable semantics of every
//! rooted path under the strongest GC stress the language surface can express
//! today, and it starts failing the day a minor-evacuating configuration
//! becomes reachable from compiled code. It is not evidence that the pre-fix
//! runtime was reproducibly wrong; the audit in #6943 is.
//!
//! Coverage: `js_object_define_property` (ordinary / closure / typed-array
//! arms), `js_object_get_own_property_descriptor` (ordinary / closure /
//! typed-array / class-object / string-primitive arms),
//! `js_object_get_own_property_descriptors`, `obj_value_has_own_key`,
//! `obj_value_attrs`, `reflect_getter_closure_bits`,
//! `array_length_reflect_define`, `typed_array_own_index`,
//! `typed_array_define_own_property`, `js_object_from_entries`,
//! `js_object_has_own`, `js_object_property_is_enumerable`, the
//! `ordinary_set_with_receiver` class-instance store fast path,
//! `js_class_register_static_symbol`'s non-symbol arm, and the
//! `globalThis`-by-name helpers in `error.rs`.

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
    // suite links regardless (same shim as the #6655 / #6935 suites).
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

/// Shared prelude. `heavyKey` is the object-shaped key whose `toString` runs
/// user JS that allocates and collects — that is the arm of `js_string_coerce`
/// that can run arbitrary code. `numericKey` and `ssoKey` cover the two
/// allocate-only arms (stringify a Number, materialize an SSO short string),
/// which are the shapes a real program hits far more often.
const PRELUDE: &str = r#"
function churnAndCollect(): void {
  let sink = 0;
  for (let i = 0; i < 20000; i++) {
    const tmp = { i, s: "pad" + i };
    sink += tmp.s.length > 0 ? 1 : 0;
  }
  if (sink !== 20000) throw new Error("churn miscounted");
  (globalThis as any).gc?.();
  // Refill: evacuation leaves the vacated region intact-but-dead, so a stale
  // read right after a copy usually still finds the original bytes. Re-filling
  // gives that region a chance to be handed out and overwritten.
  for (let i = 0; i < 20000; i++) {
    const tmp2 = { a: i, b: "fill" + i, c: [i, i + 1] };
    sink += tmp2.c[0] >= 0 ? 1 : 0;
  }
  if (sink !== 40000) throw new Error("refill miscounted");
}

// Keeps receivers / payloads reachable from a real GC root. An object reachable
// only through the (unrooted) raw local is DEAD at the collection and would
// merely be swept, so a stale read might find intact bytes. Reachable objects
// are EVACUATED — the address genuinely changes and every rooted holder is
// rewritten, while the raw local is not.
const keepalive: any[] = [];

// Object key: `js_string_coerce` -> `js_jsvalue_to_string` -> user `toString`.
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

// The STORED VALUE: a movable heap object carrying an identity we read back
// after a further collection.
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

/// `Object.defineProperty` / `Object.defineProperties`: the receiver, the
/// descriptor object and the already-dereferenced header (plain `ObjectHeader`,
/// closure pointer, TypedArray address) are all raw across the key coercion,
/// and the descriptor's `value` is written INTO the receiver afterwards.
#[test]
fn define_property_operands_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// --- ordinary object arm (define_property.rs, the `obj` header local) ---
const o1: any = receiver();
Object.defineProperty(o1, heavyKey("p1"), {
  value: payload(1),
  writable: true,
  enumerable: true,
  configurable: true,
});
churnAndCollect();
checkPayload("define-ordinary", o1.p1, 1);
check("define-ordinary-enumerable", Object.keys(o1).indexOf("p1") >= 0, true);

// A NUMBER key stringifies (allocates) without running user JS — the arm a
// real program hits constantly.
const o2: any = receiver();
Object.defineProperty(o2, 12345, { value: payload(2), configurable: true });
churnAndCollect();
checkPayload("define-numeric-key", o2[12345], 2);

// --- closure arm (the `closure_ptr` local) ---
function fn1(): number { return 1; }
keepalive.push(fn1);
Object.defineProperty(fn1, heavyKey("meta"), {
  value: payload(3),
  configurable: true,
});
churnAndCollect();
checkPayload("define-on-closure", (fn1 as any).meta, 3);
check("define-on-closure-callable", fn1(), 1);

// --- typed-array arm (the `addr` local) ---
const ta = new Int32Array([7, 8, 9]);
keepalive.push(ta);
Object.defineProperty(ta, heavyKey("label"), {
  value: payload(4),
  configurable: true,
});
churnAndCollect();
checkPayload("define-on-typed-array", (ta as any).label, 4);
check("typed-array-elements-intact", ta[0] + ta[1] + ta[2], 24);

// A CANONICAL numeric index on a typed array takes the Integer-Indexed exotic
// define — `typed_array_define_own_property`, whose view address was raw.
Object.defineProperty(ta, 1, { value: 42 });
churnAndCollect();
check("typed-array-index-define", ta[1], 42);

// --- non-configurable redefine must still be rejected after a collection ---
const frozenish: any = receiver();
Object.defineProperty(frozenish, "locked", {
  value: 1,
  configurable: false,
  writable: false,
});
let threw = false;
try {
  Object.defineProperty(frozenish, heavyKey("locked"), { value: 2 });
} catch (e) {
  threw = true;
}
churnAndCollect();
check("nonconfigurable-redefine-rejected", threw, true);
check("nonconfigurable-value-intact", frozenish.locked, 1);

// --- Object.defineProperties over a bag of descriptors ---
const bag: any = receiver();
Object.defineProperties(bag, {
  x: { value: payload(5), enumerable: true, configurable: true },
  y: { value: payload(6), enumerable: true, configurable: true },
});
churnAndCollect();
checkPayload("define-properties-x", bag.x, 5);
checkPayload("define-properties-y", bag.y, 6);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "a defineProperty path used a stale receiver or stored a stale value \
         under forced evacuation"
    );
}

/// `Object.getOwnPropertyDescriptor(s)`: every arm resolves the receiver's raw
/// header (plain object, closure, TypedArray view, class object, string
/// primitive) before the key coercion and dereferences it afterwards. The
/// plural form additionally stores each descriptor INTO a fresh result object.
#[test]
fn descriptor_reads_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// --- ordinary object arm ---
const o1: any = receiver();
o1.here = payload(10);
churnAndCollect();
const d1 = Object.getOwnPropertyDescriptor(o1, heavyKey("here"));
churnAndCollect();
checkPayload("gopd-ordinary", d1 === undefined ? undefined : d1.value, 10);
check("gopd-missing-key", Object.getOwnPropertyDescriptor(o1, heavyKey("nope")), undefined);

// --- closure arm (the `ptr` local drives every branch below the coercion) ---
function fn1(a: number, b: number): number { return a + b; }
keepalive.push(fn1);
churnAndCollect();
const dLen = Object.getOwnPropertyDescriptor(fn1, heavyKey("length"));
churnAndCollect();
check("gopd-closure-length", dLen === undefined ? undefined : dLen.value, 2);
const dName = Object.getOwnPropertyDescriptor(fn1, heavyKey("name"));
churnAndCollect();
check("gopd-closure-name", dName === undefined ? undefined : dName.value, "fn1");

// --- typed-array arm ---
const ta = new Int32Array([3, 4, 5]);
keepalive.push(ta);
churnAndCollect();
const dIdx = Object.getOwnPropertyDescriptor(ta, heavyKey("1"));
churnAndCollect();
check("gopd-typed-array-index", dIdx === undefined ? undefined : dIdx.value, 4);
check("gopd-typed-array-oob", Object.getOwnPropertyDescriptor(ta, heavyKey("99")), undefined);

// --- string primitive arm (`string_primitive_descriptor`, whose `str_value`
// receiver is itself a movable heap string) ---
const s: any = "abcdef" + String(keepalive.length % 1);
keepalive.push(s);
churnAndCollect();
const dChar = Object.getOwnPropertyDescriptor(s, heavyKey("2"));
churnAndCollect();
check("gopd-string-index", dChar === undefined ? undefined : dChar.value, "c");
const dSLen = Object.getOwnPropertyDescriptor(s, heavyKey("length"));
churnAndCollect();
check("gopd-string-length", dSLen === undefined ? undefined : dSLen.value, 6);

// --- class-object static arm ---
class C {
  static stat(): number { return 5; }
}
keepalive.push(C);
churnAndCollect();
const dStat = Object.getOwnPropertyDescriptor(C, heavyKey("stat"));
churnAndCollect();
check("gopd-class-static-present", typeof dStat === "object" && dStat !== null, true);
check(
  "gopd-class-static-callable",
  dStat === undefined ? undefined : typeof dStat.value,
  "function"
);

// --- getOwnPropertyDescriptors: result receiver + stored descriptor ---
const multi: any = receiver();
multi.a = payload(11);
multi.b = payload(12);
multi.c = payload(13);
churnAndCollect();
const all: any = Object.getOwnPropertyDescriptors(multi);
churnAndCollect();
checkPayload("gopds-a", all.a === undefined ? undefined : all.a.value, 11);
checkPayload("gopds-b", all.b === undefined ? undefined : all.b.value, 12);
checkPayload("gopds-c", all.c === undefined ? undefined : all.c.value, 13);
check("gopds-seed-present", all.seed === undefined ? undefined : all.seed.value, 1);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "a getOwnPropertyDescriptor path used a stale receiver under forced evacuation"
    );
}

/// The `Reflect.*` support predicates plus the remaining string-coerced key
/// entry points: `obj_value_has_own_key` / `obj_value_attrs` (whose stale
/// receiver ADDRESS silently misses the descriptor side table rather than
/// crashing), `array_length_reflect_define`, `reflect_getter_closure_bits`,
/// `Object.fromEntries`, and the own-property predicates.
#[test]
fn reflect_and_own_key_paths_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// --- Reflect.defineProperty -> obj_value_has_own_key + obj_value_attrs ---
const o1: any = receiver();
check(
  "reflect-define-new",
  Reflect.defineProperty(o1, heavyKey("fresh"), {
    value: payload(20),
    configurable: true,
  }),
  true
);
churnAndCollect();
checkPayload("reflect-define-new-value", o1.fresh, 20);

// A non-configurable existing property must make the redefine report false.
// `obj_value_attrs` keys the side table by the receiver's ADDRESS; a stale one
// misses and would wrongly report the all-true default, letting this through.
Object.defineProperty(o1, "pinned", { value: 1, configurable: false });
churnAndCollect();
check(
  "reflect-define-nonconfigurable",
  Reflect.defineProperty(o1, heavyKey("pinned"), { value: 2 }),
  false
);
check("reflect-define-nonconfigurable-intact", o1.pinned, 1);

// Non-extensible receiver: a brand-new key must report false.
const sealed: any = receiver();
Object.preventExtensions(sealed);
churnAndCollect();
check(
  "reflect-define-non-extensible",
  Reflect.defineProperty(sealed, heavyKey("nope"), { value: 3 }),
  false
);

// --- array `length` exotic define (array_length_reflect_define) ---
const arr: any[] = [1, 2, 3, 4, 5];
keepalive.push(arr);
churnAndCollect();
check(
  "reflect-array-length-define",
  Reflect.defineProperty(arr, heavyKey("length"), { value: 2 }),
  true
);
churnAndCollect();
check("reflect-array-length-applied", arr.length, 2);
check("reflect-array-length-head", arr[0], 1);

// --- Reflect.get through an inherited accessor (reflect_getter_closure_bits,
// which walks the prototype chain with `value`/`key` raw across the coercion).
const base: any = {};
Object.defineProperty(base, "acc", {
  get(): any { return this.backing; },
  configurable: true,
});
keepalive.push(base);
const derived: any = Object.create(base);
derived.backing = payload(21);
keepalive.push(derived);
churnAndCollect();
checkPayload("reflect-get-inherited-accessor", Reflect.get(base, "acc", derived), 21);

// --- Object.fromEntries: fresh result receiver + stored value across the
// per-entry key coercion.
const built: any = Object.fromEntries([
  [heavyKey("e1"), payload(22)],
  [heavyKey("e2"), payload(23)],
  [24680, payload(24)],
]);
keepalive.push(built);
churnAndCollect();
checkPayload("from-entries-e1", built.e1, 22);
checkPayload("from-entries-e2", built.e2, 23);
checkPayload("from-entries-numeric", built[24680], 24);

// --- own-property predicates (js_object_has_own / propertyIsEnumerable) ---
const probe: any = receiver();
probe.present = payload(25);
churnAndCollect();
check("hasOwn-object-key", Object.hasOwn(probe, heavyKey("present")), true);
check("hasOwn-object-key-miss", Object.hasOwn(probe, heavyKey("absent")), false);
check("hasOwnProperty-object-key", probe.hasOwnProperty(heavyKey("present")), true);
check(
  "propertyIsEnumerable-object-key",
  probe.propertyIsEnumerable(heavyKey("present")),
  true
);
// Numeric keys take the allocate-only arm of the same coercion.
probe[86420] = payload(26);
churnAndCollect();
check("hasOwn-numeric-key", Object.hasOwn(probe, 86420), true);
checkPayload("probe-still-intact", probe.present, 25);

// --- class-instance store fast path (ordinary_set_with_receiver): SSO short
// keys materialize onto the heap inside `js_string_coerce`, so the receiver and
// the value were raw across an allocation on the dominant `obj.f = v` path.
class Holder {
  v: any;
  constructor(v: any) { this.v = v; }
}
const h: any = new Holder(payload(27));
keepalive.push(h);
for (let i = 0; i < 200; i++) {
  h["k" + (i % 3)] = payload(30 + (i % 3));
}
churnAndCollect();
checkPayload("class-instance-store-k0", h.k0, 30);
checkPayload("class-instance-store-k1", h.k1, 31);
checkPayload("class-instance-store-k2", h.k2, 32);
checkPayload("class-instance-ctor-field", h.v, 27);

// --- class static computed field with a NON-symbol key
// (js_class_register_static_symbol's string arm stores `value` across the
// coercion).
const staticName: any = heavyKey("computedStatic");
class WithStatic {
  static [staticName] = payload(28);
}
keepalive.push(WithStatic);
churnAndCollect();
checkPayload("class-static-computed-field", (WithStatic as any).computedStatic, 28);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "a Reflect / own-key path used a stale receiver or stored a stale value \
         under forced evacuation"
    );
}
